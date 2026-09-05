//! Change detection, scheduling, and backoff for web-poll listeners.
//!
//! Polling is what makes "tell me when they release a new version" possible:
//! an inbound webhook can only be configured by whoever administers the source,
//! so watching a project you do not own is structurally a pull.
//!
//! The fingerprint is durable, which is why this is the only listener variant
//! that recovers from application downtime — the change is still visible on the
//! next poll.

use super::{
    FingerprintSource, PollMethod, WebPollTrigger, MAX_POLL_INTERVAL_SECONDS,
    MAX_POLL_MAX_BODY_BYTES, MIN_POLL_INTERVAL_SECONDS,
};
use sha2::{Digest, Sha256};

/// Ceiling on backoff so a long-dead endpoint is still retried hourly.
pub const MAX_BACKOFF_MS: u64 = 60 * 60 * 1000;

/// Failure count past which the exponent stops growing.
const MAX_BACKOFF_EXPONENT: u32 = 6;

/// The parts of an HTTP response change detection reads. The body arrives
/// already truncated to the trigger's cap by the effect layer.
#[derive(Debug, Clone, Default)]
pub struct PollResponse {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub body: Vec<u8>,
}

fn hash(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

/// Reduce a response to the value that decides whether the resource changed.
///
/// Returns `Err` when the configured extraction cannot be applied to this
/// response — a missing JSON pointer or a non-matching regex is a failure to
/// observe, not evidence of no change, and firing on it would be a lie.
pub fn fingerprint(trigger: &WebPollTrigger, response: &PollResponse) -> Result<String, String> {
    match trigger.fingerprint {
        FingerprintSource::EtagOrLastModified => {
            if let Some(etag) = response.etag.as_deref().filter(|v| !v.trim().is_empty()) {
                return Ok(format!("etag:{etag}"));
            }
            if let Some(modified) = response
                .last_modified
                .as_deref()
                .filter(|v| !v.trim().is_empty())
            {
                return Ok(format!("last-modified:{modified}"));
            }
            // A server offering neither validator still changes; fall through
            // to content rather than reporting a permanently stable resource.
            Ok(format!("body:{}", hash(&response.body)))
        }
        FingerprintSource::BodyHash => Ok(format!("body:{}", hash(&response.body))),
        FingerprintSource::JsonPointer => {
            let pointer = trigger
                .json_pointer
                .as_deref()
                .ok_or_else(|| "json_pointer fingerprints require a pointer".to_string())?;
            let body = std::str::from_utf8(&response.body)
                .map_err(|error| format!("response body is not UTF-8: {error}"))?;
            let document: serde_json::Value = serde_json::from_str(body)
                .map_err(|error| format!("response body is not JSON: {error}"))?;
            let found = document
                .pointer(pointer)
                .ok_or_else(|| format!("json pointer `{pointer}` matched nothing"))?;
            Ok(format!("json:{found}"))
        }
        FingerprintSource::Regex => {
            let pattern = trigger
                .regex
                .as_deref()
                .ok_or_else(|| "regex fingerprints require a pattern".to_string())?;
            let compiled = regex::Regex::new(pattern)
                .map_err(|error| format!("invalid regex `{pattern}`: {error}"))?;
            let body = std::str::from_utf8(&response.body)
                .map_err(|error| format!("response body is not UTF-8: {error}"))?;
            let captures = compiled
                .captures(body)
                .ok_or_else(|| format!("regex `{pattern}` matched nothing"))?;
            // Prefer the first capture group so a user can point at the part
            // that changes; fall back to the whole match when there is none.
            let value = captures
                .get(1)
                .or_else(|| captures.get(0))
                .map(|found| found.as_str())
                .unwrap_or_default();
            Ok(format!("regex:{value}"))
        }
    }
}

/// When to poll next, given how many consecutive failures precede this moment.
///
/// A flaky endpoint backs off but is never auto-disabled: unreachable is not
/// the same failure as runaway, and disabling would need a human to notice.
pub fn next_poll_epoch_ms(trigger: &WebPollTrigger, consecutive_failures: u32, now_ms: u64) -> u64 {
    let base = u64::from(trigger.interval_seconds.max(MIN_POLL_INTERVAL_SECONDS)) * 1_000;
    if consecutive_failures == 0 {
        return now_ms.saturating_add(base);
    }
    let exponent = consecutive_failures.min(MAX_BACKOFF_EXPONENT);
    let delay = base
        .saturating_mul(2_u64.saturating_pow(exponent))
        .min(MAX_BACKOFF_MS.max(base));
    now_ms.saturating_add(delay)
}

/// Ids of the poll listeners due now, seeding `next_poll_epoch_ms` for any
/// listener that has never been polled.
///
/// Mirrors `schedule::plan_tick`: it mutates the records and returns the work,
/// so the caller persists once and fires afterwards.
pub fn plan_due(listeners: &mut [super::AutomationListener], now_ms: u64) -> Vec<String> {
    let mut due = Vec::new();
    for listener in listeners.iter_mut() {
        let super::ListenerTrigger::WebPoll(trigger) = &listener.trigger else {
            continue;
        };
        if !listener.should_arm() {
            continue;
        }
        match listener.runtime.next_poll_epoch_ms {
            // A newly armed listener polls immediately: the point is to learn
            // the current fingerprint, and the first observation never fires.
            None => due.push(listener.id.clone()),
            Some(next) if next <= now_ms => due.push(listener.id.clone()),
            Some(_) => continue,
        }
        listener.runtime.next_poll_epoch_ms = Some(next_poll_epoch_ms(
            trigger,
            listener.runtime.consecutive_failures,
            now_ms,
        ));
    }
    due
}

/// Whether an observation should fire, and the fingerprint to persist.
///
/// The first observation of a listener records its fingerprint without firing.
/// Firing then would mean every new listener immediately runs its automation
/// against a resource that has not changed since the user created it.
pub fn decide(previous: Option<&str>, observed: &str) -> bool {
    match previous {
        None => false,
        Some(previous) => previous != observed,
    }
}

pub fn validate(trigger: &WebPollTrigger) -> Result<(), String> {
    let url = trigger.url.trim();
    if url.is_empty() {
        return Err("web poll listeners require a url".to_string());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!(
            "unsupported url scheme in `{url}`; expected http or https"
        ));
    }
    if trigger.interval_seconds < MIN_POLL_INTERVAL_SECONDS {
        return Err(format!(
            "interval_seconds must be at least {MIN_POLL_INTERVAL_SECONDS}"
        ));
    }
    if trigger.interval_seconds > MAX_POLL_INTERVAL_SECONDS {
        return Err(format!(
            "interval_seconds must be no greater than {MAX_POLL_INTERVAL_SECONDS}"
        ));
    }
    if trigger.max_body_bytes == 0 || trigger.max_body_bytes > MAX_POLL_MAX_BODY_BYTES {
        return Err(format!(
            "max_body_bytes must be between 1 and {MAX_POLL_MAX_BODY_BYTES}"
        ));
    }
    match trigger.fingerprint {
        FingerprintSource::JsonPointer => {
            let pointer = trigger
                .json_pointer
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "json_pointer fingerprints require a pointer".to_string())?;
            if !pointer.starts_with('/') {
                return Err(format!(
                    "invalid json pointer `{pointer}`; RFC 6901 pointers start with `/`"
                ));
            }
        }
        FingerprintSource::Regex => {
            let pattern = trigger
                .regex
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "regex fingerprints require a pattern".to_string())?;
            regex::Regex::new(pattern)
                .map_err(|error| format!("invalid regex `{pattern}`: {error}"))?;
        }
        _ => {}
    }
    if matches!(trigger.method, PollMethod::Head)
        && !matches!(trigger.fingerprint, FingerprintSource::EtagOrLastModified)
    {
        return Err(
            "HEAD requests return no body, so they only support the etag_or_last_modified fingerprint"
                .to_string(),
        );
    }
    for name in trigger.headers.keys() {
        if name.trim().is_empty() {
            return Err("request header names must not be empty".to_string());
        }
        if name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("cookie") {
            return Err(format!(
                "`{name}` is a credential; store it as a listener secret instead of in the listener config"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listeners::test_support::listener;
    use crate::listeners::{ListenerTrigger, DEFAULT_POLL_MAX_BODY_BYTES};
    use std::collections::BTreeMap;

    fn trigger(fingerprint: FingerprintSource) -> WebPollTrigger {
        WebPollTrigger {
            url: "https://example.invalid/releases".into(),
            interval_seconds: 300,
            method: PollMethod::Get,
            headers: BTreeMap::new(),
            fingerprint,
            json_pointer: None,
            regex: None,
            max_body_bytes: DEFAULT_POLL_MAX_BODY_BYTES,
        }
    }

    #[test]
    fn etag_is_preferred_then_last_modified_then_body() {
        let subject = trigger(FingerprintSource::EtagOrLastModified);
        let full = PollResponse {
            etag: Some("W/\"abc\"".into()),
            last_modified: Some("Wed, 01 Jan 2026 00:00:00 GMT".into()),
            body: b"hello".to_vec(),
        };
        assert_eq!(fingerprint(&subject, &full).unwrap(), "etag:W/\"abc\"");

        let no_etag = PollResponse {
            etag: None,
            ..full.clone()
        };
        assert!(fingerprint(&subject, &no_etag)
            .unwrap()
            .starts_with("last-modified:"));

        let neither = PollResponse {
            etag: None,
            last_modified: None,
            body: b"hello".to_vec(),
        };
        assert!(fingerprint(&subject, &neither)
            .unwrap()
            .starts_with("body:"));
    }

    #[test]
    fn a_json_pointer_tracks_a_release_tag_and_ignores_unrelated_churn() {
        let mut subject = trigger(FingerprintSource::JsonPointer);
        subject.json_pointer = Some("/0/tag_name".into());

        let first = PollResponse {
            body: br#"[{"tag_name":"v1.2.0","published_at":"2026-01-01"}]"#.to_vec(),
            ..PollResponse::default()
        };
        let churn = PollResponse {
            body: br#"[{"tag_name":"v1.2.0","published_at":"2026-01-02"}]"#.to_vec(),
            ..PollResponse::default()
        };
        let release = PollResponse {
            body: br#"[{"tag_name":"v1.3.0","published_at":"2026-01-03"}]"#.to_vec(),
            ..PollResponse::default()
        };

        let baseline = fingerprint(&subject, &first).unwrap();
        assert!(!decide(
            Some(&baseline),
            &fingerprint(&subject, &churn).unwrap()
        ));
        assert!(decide(
            Some(&baseline),
            &fingerprint(&subject, &release).unwrap()
        ));
    }

    #[test]
    fn a_missing_pointer_is_a_failure_to_observe_not_a_change() {
        let mut subject = trigger(FingerprintSource::JsonPointer);
        subject.json_pointer = Some("/0/tag_name".into());
        let empty = PollResponse {
            body: b"[]".to_vec(),
            ..PollResponse::default()
        };
        assert!(fingerprint(&subject, &empty).is_err());
    }

    #[test]
    fn a_regex_fingerprint_uses_the_first_capture_group() {
        let mut subject = trigger(FingerprintSource::Regex);
        subject.regex = Some(r"version\s+([0-9.]+)".into());
        let response = PollResponse {
            body: b"latest version 4.11.2 released".to_vec(),
            ..PollResponse::default()
        };
        assert_eq!(fingerprint(&subject, &response).unwrap(), "regex:4.11.2");
    }

    #[test]
    fn the_first_observation_records_without_firing() {
        assert!(!decide(None, "etag:abc"));
        assert!(!decide(Some("etag:abc"), "etag:abc"));
        assert!(decide(Some("etag:abc"), "etag:def"));
    }

    #[test]
    fn failures_back_off_exponentially_up_to_the_ceiling() {
        let subject = trigger(FingerprintSource::BodyHash);
        let healthy = next_poll_epoch_ms(&subject, 0, 0);
        assert_eq!(healthy, 300_000);

        let once = next_poll_epoch_ms(&subject, 1, 0);
        assert_eq!(once, 600_000);

        let many = next_poll_epoch_ms(&subject, 50, 0);
        assert!(many <= MAX_BACKOFF_MS, "backoff must stay bounded: {many}");
        assert!(many >= healthy);
    }

    #[test]
    fn a_new_poll_listener_is_due_immediately_and_then_paced() {
        let mut listeners = vec![listener(
            "poll",
            ListenerTrigger::WebPoll(trigger(FingerprintSource::BodyHash)),
        )];
        assert_eq!(plan_due(&mut listeners, 1_000), vec!["poll".to_string()]);
        assert_eq!(listeners[0].runtime.next_poll_epoch_ms, Some(301_000));
        assert!(plan_due(&mut listeners, 2_000).is_empty());
        assert_eq!(plan_due(&mut listeners, 301_000), vec!["poll".to_string()]);
    }

    #[test]
    fn a_disabled_listener_is_never_due() {
        let mut listeners = vec![listener(
            "poll",
            ListenerTrigger::WebPoll(trigger(FingerprintSource::BodyHash)),
        )];
        listeners[0].runtime.disabled_reason = Some("ceiling".into());
        assert!(plan_due(&mut listeners, 1_000).is_empty());
    }

    #[test]
    fn credential_headers_are_refused_in_the_inspectable_config() {
        let mut subject = trigger(FingerprintSource::BodyHash);
        subject
            .headers
            .insert("Authorization".into(), "Bearer hunter2".into());
        let error = validate(&subject).unwrap_err();
        assert!(error.contains("listener secret"), "{error}");
    }

    #[test]
    fn head_requests_cannot_use_a_body_fingerprint() {
        let mut subject = trigger(FingerprintSource::BodyHash);
        subject.method = PollMethod::Head;
        assert!(validate(&subject).unwrap_err().contains("HEAD"));
    }

    #[test]
    fn the_poll_interval_floor_is_enforced() {
        let mut subject = trigger(FingerprintSource::BodyHash);
        subject.interval_seconds = 5;
        assert!(validate(&subject)
            .unwrap_err()
            .contains(&MIN_POLL_INTERVAL_SECONDS.to_string()));
    }

    #[test]
    fn a_non_http_url_is_refused() {
        let mut subject = trigger(FingerprintSource::BodyHash);
        subject.url = "file:///etc/passwd".into();
        assert!(validate(&subject).unwrap_err().contains("scheme"));
    }
}

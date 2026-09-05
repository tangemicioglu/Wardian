//! Authentication and delivery identity for inbound webhook listeners.
//!
//! A webhook peer is anonymous until it proves a shared secret, so everything
//! here runs *before* a payload is trusted. The verification math is pure and
//! lives in core precisely so it can be tested without a server.

use super::{WebhookAuth, WebhookTrigger, MAX_WEBHOOK_MAX_BODY_BYTES};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Header a sender may set to make retries idempotent. GitHub's
/// `X-GitHub-Delivery` is accepted as a fallback because it means the same
/// thing and costs nothing to honor.
pub const DELIVERY_HEADER: &str = "x-wardian-delivery";
pub const GITHUB_DELIVERY_HEADER: &str = "x-github-delivery";

/// Default header carrying an HMAC signature, matching GitHub and Stripe.
pub const DEFAULT_SIGNATURE_HEADER: &str = "x-hub-signature-256";

/// Header carrying a shared bearer token when `Authorization` is inconvenient.
pub const TOKEN_HEADER: &str = "x-wardian-token";

pub const MAX_PATH_SEGMENT_LEN: usize = 64;

/// Why a delivery was refused. The reason is recorded on the listener so an
/// unfiring webhook is diagnosable, but it is never returned to the caller in
/// detail — that would let an unauthenticated peer probe the configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookRejection {
    MissingCredential,
    InvalidSignature,
    BodyTooLarge { limit: u32 },
}

impl WebhookRejection {
    pub fn reason(&self) -> String {
        match self {
            WebhookRejection::MissingCredential => {
                "delivery carried no credential header".to_string()
            }
            WebhookRejection::InvalidSignature => {
                "delivery credential did not match the listener secret".to_string()
            }
            WebhookRejection::BodyTooLarge { limit } => {
                format!("delivery body exceeded the {limit} byte limit")
            }
        }
    }
}

/// Case-insensitive header lookup over whatever shape the server hands us.
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Stable identity for one delivery, so a sender's retry becomes the same run
/// rather than a second one.
///
/// Falling back to a body hash means a sender with no delivery header still
/// gets idempotency for an identical retried payload, which is the common
/// retry shape.
pub fn delivery_id(headers: &[(String, String)], body: &[u8]) -> String {
    for name in [DELIVERY_HEADER, GITHUB_DELIVERY_HEADER] {
        if let Some(value) = header(headers, name)
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return value.to_string();
        }
    }
    format!("body:{:x}", Sha256::digest(body))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    // `ct_eq` is only constant-time for equal-length inputs; the length
    // comparison itself leaks nothing an attacker cannot already measure.
    left.len() == right.len() && bool::from(left.ct_eq(right))
}

/// Hex-encoded `HMAC-SHA256` of `body` under `secret`.
pub fn sign(secret: &str, body: &[u8]) -> Result<String, String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|error| format!("could not build signature: {error}"))?;
    mac.update(body);
    Ok(format!("{:x}", mac.finalize().into_bytes()))
}

/// Authenticate a delivery against the listener's secret.
///
/// Order matters: the size cap is checked first so an oversized body is
/// refused without hashing it, and the comparison is constant-time so a
/// mismatched signature leaks nothing about how far it matched.
pub fn authenticate(
    trigger: &WebhookTrigger,
    secret: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<(), WebhookRejection> {
    if body.len() > trigger.max_body_bytes as usize {
        return Err(WebhookRejection::BodyTooLarge {
            limit: trigger.max_body_bytes,
        });
    }
    match trigger.auth {
        WebhookAuth::Token => {
            let presented = header(headers, TOKEN_HEADER)
                .map(str::trim)
                .or_else(|| {
                    header(headers, "authorization")
                        .and_then(|value| value.trim().strip_prefix("Bearer "))
                        .map(str::trim)
                })
                .filter(|value| !value.is_empty())
                .ok_or(WebhookRejection::MissingCredential)?;
            if constant_time_eq(presented.as_bytes(), secret.as_bytes()) {
                Ok(())
            } else {
                Err(WebhookRejection::InvalidSignature)
            }
        }
        WebhookAuth::HmacSha256 => {
            let header_name = trigger
                .signature_header
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(DEFAULT_SIGNATURE_HEADER);
            let presented = header(headers, header_name)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or(WebhookRejection::MissingCredential)?;
            // GitHub and Stripe prefix the digest with the algorithm.
            let presented = presented.strip_prefix("sha256=").unwrap_or(presented);
            let expected = sign(secret, body).map_err(|_| WebhookRejection::InvalidSignature)?;
            if constant_time_eq(
                presented.to_ascii_lowercase().as_bytes(),
                expected.as_bytes(),
            ) {
                Ok(())
            } else {
                Err(WebhookRejection::InvalidSignature)
            }
        }
    }
}

/// URL path segments are stricter than filesystem components: no dots, so a
/// segment can never be read as traversal by any layer downstream.
pub fn validate_path_segment(segment: &str) -> Result<(), String> {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return Err("webhook listeners require a path segment".to_string());
    }
    if trimmed.len() > MAX_PATH_SEGMENT_LEN {
        return Err(format!(
            "webhook path segment must be at most {MAX_PATH_SEGMENT_LEN} characters"
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(format!(
            "invalid webhook path segment `{trimmed}`; use letters, digits, `-`, and `_`"
        ));
    }
    Ok(())
}

pub fn validate(trigger: &WebhookTrigger) -> Result<(), String> {
    validate_path_segment(&trigger.path_segment)?;
    if trigger.max_body_bytes == 0 || trigger.max_body_bytes > MAX_WEBHOOK_MAX_BODY_BYTES {
        return Err(format!(
            "max_body_bytes must be between 1 and {MAX_WEBHOOK_MAX_BODY_BYTES}"
        ));
    }
    if let Some(name) = trigger.signature_header.as_deref().map(str::trim) {
        if !name.is_empty()
            && !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            return Err(format!("invalid signature header name `{name}`"));
        }
    }
    Ok(())
}

/// Refuse two listeners claiming the same URL path, which would otherwise make
/// which one fires depend on iteration order.
pub fn ensure_unique_path(
    listeners: &[super::AutomationListener],
    candidate_id: &str,
    segment: &str,
) -> Result<(), String> {
    let clash = listeners.iter().find(|listener| {
        listener.id != candidate_id
            && matches!(
                &listener.trigger,
                super::ListenerTrigger::Webhook(existing)
                    if existing.path_segment.eq_ignore_ascii_case(segment.trim())
            )
    });
    match clash {
        Some(listener) => Err(format!(
            "webhook path `{segment}` is already used by listener `{}`",
            listener.name
        )),
        None => Ok(()),
    }
}

/// Bind settings for the inbound webhook server.
///
/// There is no separate enable flag: an enabled webhook listener *is* the
/// intent to receive deliveries, and making a user also flip a server switch
/// only produces a listener that looks broken for a reason nothing surfaces.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WebhookGatewayConfig {
    #[serde(default = "default_gateway_schema")]
    pub schema: u8,
    #[serde(default = "default_gateway_host")]
    pub host: String,
    #[serde(default = "default_gateway_port")]
    pub port: u16,
}

impl Default for WebhookGatewayConfig {
    fn default() -> Self {
        Self {
            schema: default_gateway_schema(),
            host: default_gateway_host(),
            port: default_gateway_port(),
        }
    }
}

fn default_gateway_schema() -> u8 {
    1
}
fn default_gateway_host() -> String {
    "127.0.0.1".to_string()
}
fn default_gateway_port() -> u16 {
    8787
}

/// v1 binds loopback only, matching the remote gateway's posture. External
/// reach is the user's tunnel, which keeps the exposure decision explicit
/// rather than a config field someone flips without meaning to.
pub fn is_loopback_host(host: &str) -> bool {
    matches!(host.trim(), "127.0.0.1" | "localhost" | "::1")
}

pub fn validate_gateway_config(config: &WebhookGatewayConfig) -> Result<(), String> {
    if !is_loopback_host(&config.host) {
        return Err(format!(
            "webhook gateway must bind to loopback; `{}` is not a loopback host",
            config.host
        ));
    }
    if config.port == 0 {
        return Err("webhook gateway port must not be zero".to_string());
    }
    Ok(())
}

/// Read gateway settings, falling back to defaults so a missing file is a
/// working configuration rather than a disabled feature.
pub fn load_gateway_config() -> WebhookGatewayConfig {
    crate::paths::listener_gateway_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

pub fn save_gateway_config(config: &WebhookGatewayConfig) -> std::io::Result<()> {
    validate_gateway_config(config)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let path = crate::paths::listener_gateway_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Wardian home is unavailable")
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(config)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)
}

/// The URL a sender should be pointed at.
pub fn webhook_url(config: &WebhookGatewayConfig, path_segment: &str) -> String {
    let host = if config.host.trim() == "::1" {
        "[::1]".to_string()
    } else {
        config.host.trim().to_string()
    };
    format!("http://{host}:{}/hooks/{path_segment}", config.port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listeners::test_support::listener;
    use crate::listeners::{ListenerTrigger, DEFAULT_WEBHOOK_MAX_BODY_BYTES};

    fn trigger(auth: WebhookAuth) -> WebhookTrigger {
        WebhookTrigger {
            path_segment: "ci".into(),
            auth,
            signature_header: None,
            max_body_bytes: DEFAULT_WEBHOOK_MAX_BODY_BYTES,
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn a_valid_hmac_signature_authenticates_with_or_without_the_algorithm_prefix() {
        let subject = trigger(WebhookAuth::HmacSha256);
        let body = br#"{"action":"published"}"#;
        let digest = sign("s3cret", body).unwrap();

        for presented in [digest.clone(), format!("sha256={digest}")] {
            let sent = headers(&[("X-Hub-Signature-256", &presented)]);
            assert!(authenticate(&subject, "s3cret", &sent, body).is_ok());
        }
    }

    #[test]
    fn a_wrong_secret_is_refused() {
        let subject = trigger(WebhookAuth::HmacSha256);
        let body = b"payload";
        let sent = headers(&[("X-Hub-Signature-256", &sign("other", body).unwrap())]);
        assert_eq!(
            authenticate(&subject, "s3cret", &sent, body),
            Err(WebhookRejection::InvalidSignature)
        );
    }

    #[test]
    fn a_missing_credential_is_distinguished_from_a_wrong_one() {
        let subject = trigger(WebhookAuth::HmacSha256);
        assert_eq!(
            authenticate(&subject, "s3cret", &headers(&[]), b"payload"),
            Err(WebhookRejection::MissingCredential)
        );
    }

    #[test]
    fn an_oversized_body_is_refused_before_the_signature_is_computed() {
        let mut subject = trigger(WebhookAuth::HmacSha256);
        subject.max_body_bytes = 8;
        let body = vec![b'x'; 64];
        assert_eq!(
            authenticate(&subject, "s3cret", &headers(&[]), &body),
            Err(WebhookRejection::BodyTooLarge { limit: 8 })
        );
    }

    #[test]
    fn token_auth_accepts_either_carrier_header() {
        let subject = trigger(WebhookAuth::Token);
        let body = b"payload";
        for sent in [
            headers(&[("X-Wardian-Token", "s3cret")]),
            headers(&[("Authorization", "Bearer s3cret")]),
        ] {
            assert!(authenticate(&subject, "s3cret", &sent, body).is_ok());
        }
        assert!(authenticate(
            &subject,
            "s3cret",
            &headers(&[("X-Wardian-Token", "wrong")]),
            body
        )
        .is_err());
    }

    #[test]
    fn a_retried_delivery_keeps_its_identity() {
        let sent = headers(&[("X-GitHub-Delivery", "abc-123")]);
        assert_eq!(delivery_id(&sent, b"first"), "abc-123");
        assert_eq!(
            delivery_id(&sent, b"different body"),
            "abc-123",
            "the sender's delivery id wins over the body"
        );
    }

    #[test]
    fn a_header_less_sender_still_gets_body_idempotency() {
        let identical = delivery_id(&headers(&[]), b"payload");
        assert_eq!(identical, delivery_id(&headers(&[]), b"payload"));
        assert_ne!(identical, delivery_id(&headers(&[]), b"other"));
    }

    #[test]
    fn path_segments_reject_traversal_and_separators() {
        for bad in ["", "..", "a/b", "a.b", "hook name", &"x".repeat(65)] {
            assert!(
                validate_path_segment(bad).is_err(),
                "{bad} should be refused"
            );
        }
        assert!(validate_path_segment("github-releases_1").is_ok());
    }

    #[test]
    fn a_non_loopback_bind_is_refused() {
        let mut config = WebhookGatewayConfig::default();
        assert!(validate_gateway_config(&config).is_ok());
        config.host = "0.0.0.0".into();
        assert!(validate_gateway_config(&config)
            .unwrap_err()
            .contains("loopback"));
    }

    #[test]
    fn an_ipv6_loopback_url_is_bracketed() {
        let config = WebhookGatewayConfig {
            host: "::1".into(),
            port: 9000,
            ..WebhookGatewayConfig::default()
        };
        assert_eq!(webhook_url(&config, "ci"), "http://[::1]:9000/hooks/ci");
    }

    #[test]
    fn two_listeners_cannot_claim_the_same_path() {
        let existing = vec![listener(
            "one",
            ListenerTrigger::Webhook(trigger(WebhookAuth::Token)),
        )];
        assert!(ensure_unique_path(&existing, "two", "ci").is_err());
        assert!(
            ensure_unique_path(&existing, "two", "CI").is_err(),
            "path matching is case-insensitive"
        );
        assert!(ensure_unique_path(&existing, "one", "ci").is_ok());
        assert!(ensure_unique_path(&existing, "two", "deploy").is_ok());
    }
}

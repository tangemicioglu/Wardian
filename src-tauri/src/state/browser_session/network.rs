//! The per-session network ledger.
//!
//! Folding protocol events into records is kept free of I/O so the rules can be
//! tested directly, the same arrangement [`super::snapshot`] uses for refs.
//!
//! The ledger is deliberately *not* cleared by navigation. Console entries
//! belong to the page that produced them, but the document request that starts
//! a navigation is emitted before the `Page.frameNavigated` that would clear
//! it — so clearing on navigation would reliably delete the single record an
//! agent is most likely to be looking for. `network --clear` is explicit
//! instead.

use std::collections::{BTreeMap, VecDeque};

use serde_json::Value;
use wardian_core::browser::{
    NetworkEntry, MAX_NETWORK_HEADERS, MAX_NETWORK_HEADER_CHARS, MAX_NETWORK_URL_CHARS,
    NETWORK_BUFFER,
};

/// One request, with the headers a listing leaves out.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkRecord {
    pub entry: NetworkEntry,
    pub request_headers: BTreeMap<String, String>,
    pub response_headers: BTreeMap<String, String>,
    /// Monotonic protocol timestamp at request start, in seconds.
    ///
    /// Kept off the wire: it is only ever used to derive a duration, and a
    /// browser-relative monotonic clock means nothing to a caller.
    started_at: Option<f64>,
}

/// A bounded ring of the requests one session has made.
#[derive(Debug, Default)]
pub struct NetworkLedger {
    records: VecDeque<NetworkRecord>,
}

impl NetworkLedger {
    /// Folds one protocol event into the ledger.
    ///
    /// Unknown methods are ignored rather than refused: the pump forwards
    /// everything in the `Network` domain, and the domain is larger than the
    /// part this ledger models.
    pub fn apply(&mut self, method: &str, params: &Value) {
        let Some(request_id) = params.get("requestId").and_then(Value::as_str) else {
            return;
        };
        match method {
            "Network.requestWillBeSent" => self.begin(request_id, params),
            "Network.responseReceived" => self.respond(request_id, params),
            "Network.requestServedFromCache" => {
                if let Some(record) = self.latest_mut(request_id) {
                    record.entry.from_cache = true;
                }
            }
            "Network.loadingFinished" => {
                let length = params
                    .get("encodedDataLength")
                    .and_then(Value::as_f64)
                    .map(|length| length.max(0.0) as u64);
                let finished_at = params.get("timestamp").and_then(Value::as_f64);
                if let Some(record) = self.latest_mut(request_id) {
                    if let Some(length) = length {
                        record.entry.encoded_data_length = Some(length);
                    }
                    settle(record, finished_at);
                }
            }
            "Network.loadingFailed" => {
                let reason = failure_reason(params);
                let finished_at = params.get("timestamp").and_then(Value::as_f64);
                if let Some(record) = self.latest_mut(request_id) {
                    record.entry.failure = Some(reason);
                    settle(record, finished_at);
                }
            }
            _ => {}
        }
    }

    /// Every record, oldest first.
    pub fn entries(&self) -> Vec<NetworkEntry> {
        self.records
            .iter()
            .map(|record| record.entry.clone())
            .collect()
    }

    /// The most recent record for an id, which is the one a redirect chain ends on.
    pub fn detail(&self, request_id: &str) -> Option<&NetworkRecord> {
        self.records
            .iter()
            .rev()
            .find(|record| record.entry.request_id == request_id)
    }

    pub fn clear(&mut self) {
        self.records.clear();
    }

    /// How many recorded requests failed outright or answered 4xx/5xx.
    pub fn failure_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| record.entry.is_failure())
            .count()
    }

    /// Starts a record, closing out the previous hop of a redirect chain first.
    ///
    /// A redirect reuses the request id and reports the hop it just completed
    /// as `redirectResponse`, so each hop becomes its own record rather than
    /// the chain collapsing into whichever status happened to land last.
    fn begin(&mut self, request_id: &str, params: &Value) {
        if let Some(redirect) = params.get("redirectResponse") {
            let timestamp = params.get("timestamp").and_then(Value::as_f64);
            if let Some(record) = self.latest_mut(request_id) {
                apply_response(record, redirect);
                settle(record, timestamp);
            }
        }
        let request = params.get("request");
        let url = request
            .and_then(|request| request.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (url, url_truncated) = truncate(url, MAX_NETWORK_URL_CHARS);
        let record = NetworkRecord {
            entry: NetworkEntry {
                request_id: request_id.to_string(),
                method: request
                    .and_then(|request| request.get("method"))
                    .and_then(Value::as_str)
                    .unwrap_or("GET")
                    .to_string(),
                url,
                resource_type: params
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("other")
                    .to_lowercase(),
                status: None,
                mime_type: None,
                encoded_data_length: None,
                failure: None,
                from_cache: false,
                duration_ms: None,
                url_truncated,
            },
            request_headers: headers_from(request.and_then(|request| request.get("headers"))),
            response_headers: BTreeMap::new(),
            started_at: params.get("timestamp").and_then(Value::as_f64),
        };
        if self.records.len() >= NETWORK_BUFFER {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    fn respond(&mut self, request_id: &str, params: &Value) {
        let Some(response) = params.get("response") else {
            return;
        };
        let response = response.clone();
        if let Some(record) = self.latest_mut(request_id) {
            apply_response(record, &response);
        }
    }

    /// The newest record for an id.
    ///
    /// Reverse order matters: a redirect chain leaves several records under one
    /// id, and every update after a hop belongs to the hop in progress.
    fn latest_mut(&mut self, request_id: &str) -> Option<&mut NetworkRecord> {
        self.records
            .iter_mut()
            .rev()
            .find(|record| record.entry.request_id == request_id)
    }
}

/// Copies the parts of a protocol response object the ledger keeps.
fn apply_response(record: &mut NetworkRecord, response: &Value) {
    if let Some(status) = response.get("status").and_then(Value::as_u64) {
        record.entry.status = Some(status.min(u64::from(u16::MAX)) as u16);
    }
    if let Some(mime) = response.get("mimeType").and_then(Value::as_str) {
        if !mime.is_empty() {
            record.entry.mime_type = Some(mime.to_string());
        }
    }
    if response
        .get("fromDiskCache")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        record.entry.from_cache = true;
    }
    let headers = headers_from(response.get("headers"));
    if !headers.is_empty() {
        record.response_headers = headers;
    }
}

/// Stamps a duration once a record reaches a terminal outcome.
fn settle(record: &mut NetworkRecord, finished_at: Option<f64>) {
    if let (Some(started), Some(finished)) = (record.started_at, finished_at) {
        let elapsed = (finished - started).max(0.0) * 1000.0;
        record.entry.duration_ms = Some(elapsed.round() as u64);
    }
}

/// Names why a request failed, preferring the specific reason over the generic one.
fn failure_reason(params: &Value) -> String {
    if let Some(blocked) = params
        .get("blockedReason")
        .and_then(Value::as_str)
        .filter(|reason| !reason.is_empty())
    {
        return format!("blocked: {blocked}");
    }
    if let Some(text) = params
        .get("errorText")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        return text.to_string();
    }
    if params
        .get("canceled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return "canceled".to_string();
    }
    "the request failed".to_string()
}

/// Reads a protocol headers object into a bounded, lowercased map.
///
/// Header names are case-insensitive and different Chromium versions disagree
/// on casing, so lowercasing here keeps `network <id>` output stable across
/// engines rather than making a caller guess which spelling to look for.
fn headers_from(value: Option<&Value>) -> BTreeMap<String, String> {
    let Some(object) = value.and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    object
        .iter()
        .take(MAX_NETWORK_HEADERS)
        .map(|(name, value)| {
            let text = value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string());
            let (text, truncated) = truncate(&text, MAX_NETWORK_HEADER_CHARS);
            (
                name.to_lowercase(),
                if truncated {
                    format!("{text}…")
                } else {
                    text
                },
            )
        })
        .collect()
}

/// Cuts a string to a character ceiling, reporting whether it had to.
fn truncate(value: &str, limit: usize) -> (String, bool) {
    let mut kept: String = value.chars().take(limit).collect();
    let truncated = kept.chars().count() < value.chars().count();
    if !truncated {
        kept = value.to_string();
    }
    (kept, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sent(request_id: &str, url: &str, method: &str) -> Value {
        json!({
            "requestId": request_id,
            "timestamp": 100.0,
            "type": "XHR",
            "request": {
                "url": url,
                "method": method,
                "headers": { "Accept": "application/json" },
            },
        })
    }

    #[test]
    fn a_request_becomes_a_record_with_its_type_lowercased() {
        let mut ledger = NetworkLedger::default();
        ledger.apply(
            "Network.requestWillBeSent",
            &sent("1", "https://example.com/api", "POST"),
        );
        let entries = ledger.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].method, "POST");
        assert_eq!(entries[0].resource_type, "xhr");
        assert_eq!(entries[0].status, None);
        assert_eq!(
            ledger.detail("1").expect("record").request_headers["accept"],
            "application/json"
        );
    }

    #[test]
    fn a_response_and_completion_fill_in_the_status_size_and_duration() {
        let mut ledger = NetworkLedger::default();
        ledger.apply("Network.requestWillBeSent", &sent("1", "https://a/", "GET"));
        ledger.apply(
            "Network.responseReceived",
            &json!({
                "requestId": "1",
                "response": {
                    "status": 204,
                    "mimeType": "application/json",
                    "headers": { "Content-Type": "application/json" },
                },
            }),
        );
        ledger.apply(
            "Network.loadingFinished",
            &json!({ "requestId": "1", "timestamp": 100.25, "encodedDataLength": 1234.0 }),
        );
        let entry = &ledger.entries()[0];
        assert_eq!(entry.status, Some(204));
        assert_eq!(entry.mime_type.as_deref(), Some("application/json"));
        assert_eq!(entry.encoded_data_length, Some(1234));
        assert_eq!(entry.duration_ms, Some(250));
        assert_eq!(
            ledger.detail("1").expect("record").response_headers["content-type"],
            "application/json"
        );
    }

    #[test]
    fn a_failure_records_its_reason_and_counts_as_one() {
        let mut ledger = NetworkLedger::default();
        ledger.apply("Network.requestWillBeSent", &sent("1", "https://a/", "GET"));
        ledger.apply(
            "Network.loadingFailed",
            &json!({
                "requestId": "1",
                "timestamp": 100.1,
                "errorText": "net::ERR_CONNECTION_REFUSED",
            }),
        );
        assert_eq!(
            ledger.entries()[0].failure.as_deref(),
            Some("net::ERR_CONNECTION_REFUSED")
        );
        assert_eq!(ledger.failure_count(), 1);
    }

    #[test]
    fn a_blocked_request_reports_the_block_rather_than_the_generic_error() {
        let mut ledger = NetworkLedger::default();
        ledger.apply("Network.requestWillBeSent", &sent("1", "https://a/", "GET"));
        ledger.apply(
            "Network.loadingFailed",
            &json!({
                "requestId": "1",
                "errorText": "net::ERR_BLOCKED_BY_CLIENT",
                "blockedReason": "csp",
            }),
        );
        assert_eq!(ledger.entries()[0].failure.as_deref(), Some("blocked: csp"));
    }

    #[test]
    fn a_cancellation_with_no_error_text_still_says_something_useful() {
        let mut ledger = NetworkLedger::default();
        ledger.apply("Network.requestWillBeSent", &sent("1", "https://a/", "GET"));
        ledger.apply(
            "Network.loadingFailed",
            &json!({ "requestId": "1", "canceled": true }),
        );
        assert_eq!(ledger.entries()[0].failure.as_deref(), Some("canceled"));
    }

    #[test]
    fn an_error_status_counts_as_a_failure_even_though_it_loaded() {
        let mut ledger = NetworkLedger::default();
        ledger.apply("Network.requestWillBeSent", &sent("1", "https://a/", "GET"));
        ledger.apply(
            "Network.responseReceived",
            &json!({ "requestId": "1", "response": { "status": 500 } }),
        );
        assert_eq!(ledger.failure_count(), 1);
    }

    #[test]
    fn a_redirect_keeps_each_hop_as_its_own_record() {
        let mut ledger = NetworkLedger::default();
        ledger.apply(
            "Network.requestWillBeSent",
            &sent("1", "https://example.com/old", "GET"),
        );
        let mut redirected = sent("1", "https://example.com/new", "GET");
        redirected["timestamp"] = json!(100.05);
        redirected["redirectResponse"] = json!({ "status": 301, "mimeType": "text/html" });
        ledger.apply("Network.requestWillBeSent", &redirected);
        ledger.apply(
            "Network.responseReceived",
            &json!({ "requestId": "1", "response": { "status": 200 } }),
        );

        let entries = ledger.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://example.com/old");
        assert_eq!(entries[0].status, Some(301));
        assert_eq!(entries[0].duration_ms, Some(50));
        assert_eq!(entries[1].url, "https://example.com/new");
        assert_eq!(entries[1].status, Some(200));
    }

    #[test]
    fn a_later_update_reaches_the_hop_in_progress_not_the_finished_one() {
        let mut ledger = NetworkLedger::default();
        ledger.apply("Network.requestWillBeSent", &sent("1", "https://a/", "GET"));
        let mut redirected = sent("1", "https://b/", "GET");
        redirected["redirectResponse"] = json!({ "status": 302 });
        ledger.apply("Network.requestWillBeSent", &redirected);
        ledger.apply(
            "Network.loadingFailed",
            &json!({ "requestId": "1", "errorText": "net::ERR_ABORTED" }),
        );

        let entries = ledger.entries();
        assert_eq!(entries[0].failure, None, "the completed hop is untouched");
        assert_eq!(entries[1].failure.as_deref(), Some("net::ERR_ABORTED"));
    }

    #[test]
    fn a_cache_hit_is_recorded_from_either_signal() {
        let mut ledger = NetworkLedger::default();
        ledger.apply("Network.requestWillBeSent", &sent("1", "https://a/", "GET"));
        ledger.apply(
            "Network.requestServedFromCache",
            &json!({ "requestId": "1" }),
        );
        assert!(ledger.entries()[0].from_cache);

        let mut disk = NetworkLedger::default();
        disk.apply("Network.requestWillBeSent", &sent("2", "https://b/", "GET"));
        disk.apply(
            "Network.responseReceived",
            &json!({ "requestId": "2", "response": { "status": 200, "fromDiskCache": true } }),
        );
        assert!(disk.entries()[0].from_cache);
    }

    #[test]
    fn the_ledger_drops_its_oldest_records_rather_than_growing_without_bound() {
        let mut ledger = NetworkLedger::default();
        for index in 0..NETWORK_BUFFER + 10 {
            ledger.apply(
                "Network.requestWillBeSent",
                &sent(&index.to_string(), "https://a/", "GET"),
            );
        }
        let entries = ledger.entries();
        assert_eq!(entries.len(), NETWORK_BUFFER);
        assert_eq!(entries[0].request_id, "10");
    }

    #[test]
    fn an_enormous_url_is_cut_and_says_it_was_cut() {
        let mut ledger = NetworkLedger::default();
        let url = format!("https://example.com/{}", "a".repeat(MAX_NETWORK_URL_CHARS));
        ledger.apply("Network.requestWillBeSent", &sent("1", &url, "GET"));
        let entry = &ledger.entries()[0];
        assert!(entry.url_truncated);
        assert_eq!(entry.url.chars().count(), MAX_NETWORK_URL_CHARS);
    }

    #[test]
    fn headers_are_capped_in_count_and_in_value_length() {
        let mut headers = serde_json::Map::new();
        for index in 0..MAX_NETWORK_HEADERS + 5 {
            headers.insert(format!("x-header-{index:03}"), json!("v"));
        }
        headers.insert(
            "x-long".to_string(),
            json!("v".repeat(MAX_NETWORK_HEADER_CHARS + 10)),
        );
        let mut ledger = NetworkLedger::default();
        ledger.apply(
            "Network.requestWillBeSent",
            &json!({
                "requestId": "1",
                "request": { "url": "https://a/", "method": "GET", "headers": headers },
            }),
        );
        let record = ledger.detail("1").expect("record");
        assert_eq!(record.request_headers.len(), MAX_NETWORK_HEADERS);
        for value in record.request_headers.values() {
            assert!(value.chars().count() <= MAX_NETWORK_HEADER_CHARS + 1);
        }
    }

    #[test]
    fn an_event_for_an_unknown_request_is_ignored_rather_than_inventing_a_record() {
        let mut ledger = NetworkLedger::default();
        ledger.apply(
            "Network.loadingFinished",
            &json!({ "requestId": "ghost", "encodedDataLength": 10.0 }),
        );
        ledger.apply("Network.dataReceived", &json!({ "requestId": "ghost" }));
        ledger.apply("Network.requestWillBeSent", &json!({ "timestamp": 1.0 }));
        assert!(ledger.entries().is_empty());
    }

    #[test]
    fn clearing_empties_the_ledger_and_its_failure_count() {
        let mut ledger = NetworkLedger::default();
        ledger.apply("Network.requestWillBeSent", &sent("1", "https://a/", "GET"));
        ledger.apply(
            "Network.loadingFailed",
            &json!({ "requestId": "1", "errorText": "net::ERR_FAILED" }),
        );
        ledger.clear();
        assert!(ledger.entries().is_empty());
        assert_eq!(ledger.failure_count(), 0);
        assert!(ledger.detail("1").is_none());
    }
}

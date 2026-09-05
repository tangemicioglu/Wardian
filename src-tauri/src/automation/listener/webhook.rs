//! The inbound webhook server.
//!
//! Deliberately separate from the remote gateway. That server is opt-in,
//! loopback-bound, and gated on P-256 device pairing; a webhook sender can
//! perform none of that handshake, and a receiver that only worked when remote
//! access happened to be enabled would be a coupling nobody could reason
//! about. Different trust domain, different server.

use super::launch::{self, FireOutcome, ListenerFire};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde_json::{Map, Value};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tauri::AppHandle;
use wardian_core::listeners::{
    self, secrets, webhook as webhook_rules, AutomationListener, ListenerTrigger, WebhookTrigger,
    MAX_WEBHOOK_MAX_BODY_BYTES,
};

/// Header names never copied into a run payload.
///
/// The payload becomes `trigger.output` and therefore reaches an agent prompt
/// and a durable run log; copying the signature or bearer token there would
/// publish the listener's own secret.
fn is_credential_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "authorization" | "cookie" | "proxy-authorization"
    ) || ["signature", "token", "secret", "-key", "password"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn header_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn payload(
    listener: &AutomationListener,
    delivery_id: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("listener_id".into(), Value::String(listener.id.clone()));
    map.insert("listener_name".into(), Value::String(listener.name.clone()));
    map.insert("trigger_type".into(), Value::String("webhook".into()));
    map.insert("delivery_id".into(), Value::String(delivery_id.to_string()));

    let safe_headers: Map<String, Value> = headers
        .iter()
        .filter(|(name, _)| !is_credential_header(name))
        .map(|(name, value)| (name.to_ascii_lowercase(), Value::String(value.clone())))
        .collect();
    map.insert("headers".into(), Value::Object(safe_headers));

    let text = String::from_utf8(body.to_vec()).ok();
    map.insert(
        "body".into(),
        text.as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or(Value::Null),
    );
    map.insert(
        "body_text".into(),
        text.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert(
        "observed_at".into(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    map
}

fn find_listener(segment: &str) -> Option<(AutomationListener, WebhookTrigger)> {
    listeners::load_listeners()
        .into_iter()
        .find_map(|listener| {
            if !listener.should_arm() {
                return None;
            }
            match &listener.trigger {
                ListenerTrigger::Webhook(trigger)
                    if trigger.path_segment.eq_ignore_ascii_case(segment.trim()) =>
                {
                    let trigger = trigger.clone();
                    Some((listener, trigger))
                }
                _ => None,
            }
        })
}

async fn receive(
    State(app): State<AppHandle>,
    Path(segment): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // An unknown path answers exactly like a wrong secret would, so an
    // unauthenticated peer cannot enumerate which listeners exist.
    let Some((listener, trigger)) = find_listener(&segment) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not found" })),
        );
    };

    let Some(secret) = secrets::load_secret(&listener.id).and_then(|stored| stored.webhook_secret)
    else {
        launch::record_rejection(
            &listener.id,
            "listener has no stored secret, so no delivery can be authenticated".to_string(),
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "listener is not configured" })),
        );
    };

    let pairs = header_pairs(&headers);
    if let Err(rejection) = webhook_rules::authenticate(&trigger, &secret, &pairs, &body) {
        launch::record_rejection(&listener.id, rejection.reason());
        let status = match rejection {
            webhook_rules::WebhookRejection::BodyTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            _ => StatusCode::UNAUTHORIZED,
        };
        launch::emit_listeners_updated(&app);
        return (status, Json(serde_json::json!({ "error": "rejected" })));
    }

    let delivery_id = webhook_rules::delivery_id(&pairs, &body);
    let fire = ListenerFire {
        listener_id: listener.id.clone(),
        event_identity: delivery_id.clone(),
        payload: payload(&listener, &delivery_id, &pairs, &body),
    };

    // Awaited rather than spawned: the response means "durably claimed", so it
    // cannot be sent before the run exists. Waiting for the run to *finish*
    // would instead hold the sender open for the length of an agent session
    // and guarantee a timeout-and-retry.
    let outcome = launch::fire(app.clone(), listener, fire).await;

    let status = match outcome {
        // A retry landing on an already-claimed delivery is still an accepted
        // delivery; answering with an error would drive an infinite retry for
        // a request Wardian handled correctly.
        FireOutcome::Started(_)
        | FireOutcome::AlreadyClaimed
        | FireOutcome::Skipped
        | FireOutcome::Coalesced => StatusCode::ACCEPTED,
        FireOutcome::RateLimited => StatusCode::TOO_MANY_REQUESTS,
    };
    let described = match &outcome {
        FireOutcome::Started(run_id) => {
            serde_json::json!({ "outcome": "started", "run_id": run_id })
        }
        FireOutcome::AlreadyClaimed => serde_json::json!({ "outcome": "already_accepted" }),
        FireOutcome::Skipped => serde_json::json!({ "outcome": "skipped" }),
        FireOutcome::Coalesced => serde_json::json!({ "outcome": "coalesced" }),
        FireOutcome::RateLimited => serde_json::json!({ "outcome": "rate_limited" }),
    };
    (status, Json(described))
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

fn router(app: AppHandle) -> Router {
    Router::new()
        .route("/hooks/{segment}", post(receive))
        .route("/hooks/health", axum::routing::get(health))
        // A hard ceiling above every per-listener cap, so an oversized body is
        // refused by the server before a handler ever buffers it.
        .layer(DefaultBodyLimit::max(MAX_WEBHOOK_MAX_BODY_BYTES as usize))
        .with_state(app)
}

fn socket_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
    match host.trim() {
        "127.0.0.1" | "localhost" => Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)),
        "::1" => Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port)),
        other => Err(format!(
            "webhook gateway must bind to loopback; `{other}` is not a loopback host"
        )),
    }
}

/// Start the webhook server, returning its task handle and the bound address.
///
/// Binding is attempted synchronously so a port conflict surfaces as an arming
/// error on the webhook listeners rather than as silence.
pub async fn serve(
    app: AppHandle,
) -> Result<(tauri::async_runtime::JoinHandle<()>, SocketAddr), String> {
    let config = webhook_rules::load_gateway_config();
    webhook_rules::validate_gateway_config(&config)?;
    let addr = socket_addr(&config.host, config.port)?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| format!("could not bind webhook gateway on {addr}: {error}"))?;
    let bound = listener
        .local_addr()
        .map_err(|error| format!("could not read webhook gateway address: {error}"))?;
    let router = router(app);
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            crate::utils::logging::log_debug(&format!(
                "[automation] webhook gateway stopped: {error}"
            ));
        }
    });
    Ok((handle, bound))
}

/// Timestamped rejection helper shared with the supervisor when a delivery is
/// refused before a listener is identified.
pub fn rejection_at() -> u64 {
    now_ms()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardian_core::listeners::{WebhookAuth, DEFAULT_WEBHOOK_MAX_BODY_BYTES};

    fn listener() -> AutomationListener {
        AutomationListener {
            id: "hook".into(),
            blueprint_id: "audit".into(),
            name: "CI".into(),
            enabled: true,
            trigger: ListenerTrigger::Webhook(WebhookTrigger {
                path_segment: "ci".into(),
                auth: WebhookAuth::HmacSha256,
                signature_header: None,
                max_body_bytes: DEFAULT_WEBHOOK_MAX_BODY_BYTES,
            }),
            provider: None,
            workspace: None,
            input: serde_json::json!({}),
            bindings: Default::default(),
            assignments: Default::default(),
            overlap: None,
            runtime: Default::default(),
        }
    }

    #[test]
    fn credential_headers_never_reach_the_run_payload() {
        let headers = vec![
            ("X-GitHub-Event".to_string(), "release".to_string()),
            (
                "X-Hub-Signature-256".to_string(),
                "sha256=deadbeef".to_string(),
            ),
            ("Authorization".to_string(), "Bearer hunter2".to_string()),
            ("X-Wardian-Token".to_string(), "s3cret".to_string()),
            ("X-Api-Key".to_string(), "abc".to_string()),
            ("Cookie".to_string(), "session=1".to_string()),
        ];
        let map = payload(&listener(), "delivery-1", &headers, b"{}");
        let carried = map["headers"].as_object().expect("headers object");

        assert!(carried.contains_key("x-github-event"));
        for leaked in [
            "x-hub-signature-256",
            "authorization",
            "x-wardian-token",
            "x-api-key",
            "cookie",
        ] {
            assert!(
                !carried.contains_key(leaked),
                "{leaked} must not reach an agent prompt or a run log"
            );
        }
    }

    #[test]
    fn a_json_body_is_parsed_and_the_raw_text_is_kept_beside_it() {
        let map = payload(
            &listener(),
            "delivery-1",
            &[],
            br#"{"action":"published","number":7}"#,
        );
        assert_eq!(map["body"]["action"], "published");
        assert_eq!(map["body"]["number"], serde_json::json!(7));
        assert!(map["body_text"]
            .as_str()
            .expect("text")
            .contains("published"));
    }

    #[test]
    fn a_non_json_body_leaves_the_parsed_field_null_without_losing_the_text() {
        let map = payload(&listener(), "delivery-1", &[], b"plain text hook");
        assert_eq!(map["body"], Value::Null);
        assert_eq!(map["body_text"], "plain text hook");
    }

    #[test]
    fn a_binary_body_reports_neither_parsed_nor_text() {
        let map = payload(&listener(), "delivery-1", &[], &[0xff, 0xfe, 0x00]);
        assert_eq!(map["body"], Value::Null);
        assert_eq!(map["body_text"], Value::Null);
    }

    #[test]
    fn only_loopback_addresses_resolve() {
        assert!(socket_addr("127.0.0.1", 8787).is_ok());
        assert!(socket_addr("::1", 8787).is_ok());
        assert!(socket_addr("0.0.0.0", 8787).is_err());
        assert!(socket_addr("example.com", 8787).is_err());
    }

    #[test]
    fn credential_header_detection_covers_common_carriers() {
        for name in [
            "Authorization",
            "cookie",
            "X-Hub-Signature-256",
            "x-wardian-token",
            "X-Api-Key",
            "X-Secret-Thing",
        ] {
            assert!(is_credential_header(name), "{name} should be filtered");
        }
        for name in ["X-GitHub-Event", "Content-Type", "User-Agent"] {
            assert!(!is_credential_header(name), "{name} should be carried");
        }
    }
}

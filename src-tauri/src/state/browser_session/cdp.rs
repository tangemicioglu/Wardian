//! A minimal Chrome DevTools Protocol client.
//!
//! Only what browser surfaces need: request/response correlation, flattened
//! target sessions, and an event stream. The connection owns one websocket and
//! two background tasks; every caller talks to it through [`CdpConnection`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

/// Ceiling on how long any single protocol call may take.
pub const CDP_CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Buffered protocol events. Frames are the high-volume producer, so this is
/// sized to absorb a screencast burst without dropping navigation events.
const EVENT_CHANNEL_CAPACITY: usize = 512;

/// Synthetic event published when the websocket closes.
///
/// A subscriber cannot detect closure by the channel ending: the sender lives
/// in the connection, which subscribers hold alive. Without an explicit signal
/// a crashed browser would leave every reader waiting forever.
pub const DISCONNECTED_METHOD: &str = "Wardian.disconnected";

/// A protocol event addressed to a specific target session, or to the browser.
#[derive(Debug, Clone)]
pub struct CdpEvent {
    pub session_id: Option<String>,
    pub method: String,
    pub params: Value,
}

/// Why a protocol call failed.
#[derive(Debug, Clone)]
pub enum CdpError {
    /// The websocket closed or was never established.
    Disconnected,
    /// The call exceeded [`CDP_CALL_TIMEOUT`].
    Timeout { method: String },
    /// The browser answered with an error object.
    Protocol {
        method: String,
        code: i64,
        message: String,
    },
    /// The browser answered with something this client cannot read.
    Malformed { method: String, detail: String },
}

impl std::fmt::Display for CdpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CdpError::Disconnected => write!(formatter, "the browser connection is closed"),
            CdpError::Timeout { method } => {
                write!(formatter, "{method} did not answer within {} seconds", CDP_CALL_TIMEOUT.as_secs())
            }
            CdpError::Protocol {
                method,
                code,
                message,
            } => write!(formatter, "{method} failed ({code}): {message}"),
            CdpError::Malformed { method, detail } => {
                write!(formatter, "{method} returned an unreadable response: {detail}")
            }
        }
    }
}

impl std::error::Error for CdpError {}

#[derive(Debug, Deserialize)]
struct ProtocolErrorBody {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

/// Splits one inbound protocol frame into either a command reply or an event.
///
/// Kept free of I/O so the routing rules can be tested directly.
pub(crate) enum InboundFrame {
    Reply { id: u64, result: Result<Value, (i64, String)> },
    Event(CdpEvent),
    Ignored,
}

pub(crate) fn classify_frame(text: &str) -> InboundFrame {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return InboundFrame::Ignored;
    };
    if let Some(id) = value.get("id").and_then(Value::as_u64) {
        if let Some(error) = value.get("error") {
            let body: ProtocolErrorBody =
                serde_json::from_value(error.clone()).unwrap_or(ProtocolErrorBody {
                    code: 0,
                    message: error.to_string(),
                });
            return InboundFrame::Reply {
                id,
                result: Err((body.code, body.message)),
            };
        }
        return InboundFrame::Reply {
            id,
            result: Ok(value
                .get("result")
                .cloned()
                .unwrap_or_else(|| json!({}))),
        };
    }
    match value.get("method").and_then(Value::as_str) {
        Some(method) => InboundFrame::Event(CdpEvent {
            session_id: value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string),
            method: method.to_string(),
            params: value.get("params").cloned().unwrap_or_else(|| json!({})),
        }),
        None => InboundFrame::Ignored,
    }
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, (i64, String)>>>>>;

/// An open DevTools Protocol connection to one browser process.
#[derive(Debug)]
pub struct CdpConnection {
    next_id: AtomicU64,
    /// Set when the socket closes, so later calls fail immediately instead of
    /// each waiting out the full call timeout against a dead browser.
    closed: AtomicBool,
    outbound: mpsc::UnboundedSender<Message>,
    pending: PendingMap,
    events: broadcast::Sender<CdpEvent>,
}

impl CdpConnection {
    /// Connects to a browser's websocket endpoint and starts pumping frames.
    pub async fn connect(websocket_url: &str) -> Result<Arc<Self>, CdpError> {
        let (stream, _response) = tokio_tungstenite::connect_async(websocket_url)
            .await
            .map_err(|_| CdpError::Disconnected)?;
        let (mut sink, mut source) = stream.split();
        let (outbound, mut outbound_rx) = mpsc::unbounded_channel::<Message>();
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

        let connection = Arc::new(Self {
            next_id: AtomicU64::new(1),
            closed: AtomicBool::new(false),
            outbound,
            pending: Arc::clone(&pending),
            events: events.clone(),
        });
        // The reader owns a handle so it can mark the connection closed; the
        // task ends when the socket does, so this cycle always resolves.
        let connection_closed = Arc::clone(&connection);

        tokio::spawn(async move {
            while let Some(message) = outbound_rx.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        tokio::spawn(async move {
            while let Some(Ok(message)) = source.next().await {
                let text = match message {
                    Message::Text(text) => text.to_string(),
                    Message::Binary(bytes) => match String::from_utf8(bytes.to_vec()) {
                        Ok(text) => text,
                        Err(_) => continue,
                    },
                    Message::Close(_) => break,
                    _ => continue,
                };
                match classify_frame(&text) {
                    InboundFrame::Reply { id, result } => {
                        if let Some(sender) = pending.lock().await.remove(&id) {
                            let _ = sender.send(result);
                        }
                    }
                    InboundFrame::Event(event) => {
                        let _ = events.send(event);
                    }
                    InboundFrame::Ignored => {}
                }
            }
            // Fail every in-flight call rather than leaving callers to time out
            // one by one after the socket is already gone.
            connection_closed.closed.store(true, Ordering::Release);
            for (_, sender) in pending.lock().await.drain() {
                let _ = sender.send(Err((-1, "connection closed".to_string())));
            }
            let _ = events.send(CdpEvent {
                session_id: None,
                method: DISCONNECTED_METHOD.to_string(),
                params: json!({}),
            });
        });

        Ok(connection)
    }

    /// Subscribes to every protocol event on this connection.
    pub fn subscribe(&self) -> broadcast::Receiver<CdpEvent> {
        self.events.subscribe()
    }

    /// Issues a browser-scoped command.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, CdpError> {
        self.dispatch(method, params, None).await
    }

    /// Issues a command scoped to an attached target session.
    pub async fn call_session(
        &self,
        session_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, CdpError> {
        self.dispatch(method, params, Some(session_id)).await
    }

    /// True once the websocket has closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    async fn dispatch(
        &self,
        method: &str,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<Value, CdpError> {
        if self.is_closed() {
            return Err(CdpError::Disconnected);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut envelope = json!({ "id": id, "method": method, "params": params });
        if let Some(session_id) = session_id {
            envelope["sessionId"] = json!(session_id);
        }
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);
        if self
            .outbound
            .send(Message::Text(envelope.to_string().into()))
            .is_err()
        {
            self.pending.lock().await.remove(&id);
            return Err(CdpError::Disconnected);
        }

        match timeout(CDP_CALL_TIMEOUT, receiver).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err((code, message)))) => Err(CdpError::Protocol {
                method: method.to_string(),
                code,
                message,
            }),
            Ok(Err(_)) => Err(CdpError::Disconnected),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(CdpError::Timeout {
                    method: method.to_string(),
                })
            }
        }
    }
}

/// Reads a required string field out of a protocol result.
pub fn required_str(method: &str, value: &Value, field: &str) -> Result<String, CdpError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CdpError::Malformed {
            method: method.to_string(),
            detail: format!("missing {field}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_a_successful_reply() {
        match classify_frame(r#"{"id":7,"result":{"targetId":"t1"}}"#) {
            InboundFrame::Reply { id, result } => {
                assert_eq!(id, 7);
                assert_eq!(result.expect("ok")["targetId"], "t1");
            }
            _ => panic!("expected a reply"),
        }
    }

    #[test]
    fn classifies_a_reply_with_no_result_body() {
        match classify_frame(r#"{"id":8}"#) {
            InboundFrame::Reply { id, result } => {
                assert_eq!(id, 8);
                assert_eq!(result.expect("ok"), json!({}));
            }
            _ => panic!("expected a reply"),
        }
    }

    #[test]
    fn classifies_a_protocol_error() {
        match classify_frame(r#"{"id":9,"error":{"code":-32000,"message":"nope"}}"#) {
            InboundFrame::Reply { id, result } => {
                assert_eq!(id, 9);
                let (code, message) = result.expect_err("error");
                assert_eq!(code, -32000);
                assert_eq!(message, "nope");
            }
            _ => panic!("expected a reply"),
        }
    }

    #[test]
    fn classifies_a_session_scoped_event() {
        match classify_frame(
            r#"{"method":"Page.frameNavigated","sessionId":"s1","params":{"frame":{}}}"#,
        ) {
            InboundFrame::Event(event) => {
                assert_eq!(event.method, "Page.frameNavigated");
                assert_eq!(event.session_id.as_deref(), Some("s1"));
            }
            _ => panic!("expected an event"),
        }
    }

    #[test]
    fn classifies_a_browser_scoped_event_without_a_session() {
        match classify_frame(r#"{"method":"Target.targetCreated","params":{}}"#) {
            InboundFrame::Event(event) => {
                assert_eq!(event.session_id, None);
                assert_eq!(event.params, json!({}));
            }
            _ => panic!("expected an event"),
        }
    }

    #[test]
    fn ignores_frames_that_are_neither_replies_nor_events() {
        assert!(matches!(classify_frame("not json"), InboundFrame::Ignored));
        assert!(matches!(classify_frame(r#"{"hello":1}"#), InboundFrame::Ignored));
    }

    #[test]
    fn required_str_reports_the_missing_field_by_name() {
        let error = required_str("Target.createTarget", &json!({}), "targetId")
            .expect_err("missing field");
        assert!(error.to_string().contains("missing targetId"));
    }

    #[test]
    fn a_protocol_error_names_the_failing_method() {
        let error = CdpError::Protocol {
            method: "Page.navigate".to_string(),
            code: -32000,
            message: "Cannot navigate to invalid URL".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Page.navigate failed (-32000): Cannot navigate to invalid URL"
        );
    }
}

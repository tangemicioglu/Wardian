use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wardian_core::native_transport::{
    NativeDeliveryPhase, NativeMessageEnvelope, NativeMessageOperation, NativeTransportCapabilities,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeProviderProtocol {
    ClaudeStreamJson,
    CodexAppServer,
    AntigravityStreamJson,
    OpenCodeAcp,
    PiRpc,
}

impl NativeProviderProtocol {
    pub fn for_provider(provider: &str) -> Option<Self> {
        match provider.trim().to_ascii_lowercase().as_str() {
            "claude" => Some(Self::ClaudeStreamJson),
            "codex" => Some(Self::CodexAppServer),
            "antigravity" => Some(Self::AntigravityStreamJson),
            "opencode" => Some(Self::OpenCodeAcp),
            "pi" => Some(Self::PiRpc),
            _ => None,
        }
    }

    pub fn provider(self) -> &'static str {
        match self {
            Self::ClaudeStreamJson => "claude",
            Self::CodexAppServer => "codex",
            Self::AntigravityStreamJson => "antigravity",
            Self::OpenCodeAcp => "opencode",
            Self::PiRpc => "pi",
        }
    }

    pub fn transport(self) -> &'static str {
        match self {
            Self::ClaudeStreamJson => "claude_stream_json",
            Self::CodexAppServer => "codex_app_server",
            Self::AntigravityStreamJson => "antigravity_stream_json",
            Self::OpenCodeAcp => "opencode_acp",
            Self::PiRpc => "pi_rpc",
        }
    }

    pub fn capabilities(self, protocol_version: impl Into<String>) -> NativeTransportCapabilities {
        let mut capabilities = NativeTransportCapabilities {
            provider: self.provider().to_string(),
            transport: self.transport().to_string(),
            protocol_version: protocol_version.into(),
            persistent_session: true,
            positive_turn_start: true,
            late_reconciliation: true,
            cancellation: false,
            invalidate_premise: false,
            approval_requests: false,
            max_payload_bytes: None,
            execution_timeout_ms: None,
        };
        match self {
            Self::ClaudeStreamJson => {
                capabilities.cancellation = true;
                capabilities.approval_requests = true;
            }
            Self::CodexAppServer => {
                capabilities.cancellation = true;
                capabilities.invalidate_premise = true;
                capabilities.approval_requests = true;
            }
            Self::AntigravityStreamJson => {}
            Self::OpenCodeAcp => {
                capabilities.cancellation = true;
                capabilities.approval_requests = true;
            }
            Self::PiRpc => {
                capabilities.cancellation = true;
                capabilities.invalidate_premise = true;
                capabilities.approval_requests = true;
            }
        }
        capabilities
    }

    pub fn bootstrap_requests(
        self,
        agent_id: &str,
        workspace: &str,
        provider_session_id: Option<&str>,
    ) -> Vec<Value> {
        match self {
            Self::CodexAppServer => {
                let mut requests = vec![json!({
                    "id": format!("wardian:init:{agent_id}"),
                    "method": "initialize",
                    "params": {
                        "clientInfo": {"name": "wardian", "title": "Wardian", "version": env!("CARGO_PKG_VERSION")},
                        "capabilities": {"experimentalApi": true}
                    }
                })];
                requests.push(if let Some(thread_id) = provider_session_id {
                    json!({
                        "id": format!("wardian:thread:{agent_id}"),
                        "method": "thread/resume",
                        "params": {"threadId": thread_id}
                    })
                } else {
                    json!({
                        "id": format!("wardian:thread:{agent_id}"),
                        "method": "thread/start",
                        "params": {"cwd": workspace}
                    })
                });
                requests
            }
            Self::OpenCodeAcp => {
                let mut requests = vec![json!({
                    "jsonrpc": "2.0",
                    "id": format!("wardian:init:{agent_id}"),
                    "method": "initialize",
                    "params": {
                        "protocolVersion": 1,
                        "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                        "clientInfo": {"name": "wardian", "title": "Wardian", "version": env!("CARGO_PKG_VERSION")}
                    }
                })];
                requests.push(if let Some(session_id) = provider_session_id {
                    json!({
                        "jsonrpc": "2.0",
                        "id": format!("wardian:session:{agent_id}"),
                        "method": "session/load",
                        "params": {"sessionId": session_id, "cwd": workspace, "mcpServers": []}
                    })
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": format!("wardian:session:{agent_id}"),
                        "method": "session/new",
                        "params": {"cwd": workspace, "mcpServers": []}
                    })
                });
                requests
            }
            Self::PiRpc => vec![json!({
                "id": format!("wardian:state:{agent_id}"),
                "type": "get_state"
            })],
            Self::ClaudeStreamJson | Self::AntigravityStreamJson => Vec::new(),
        }
    }

    pub fn submit_request(
        self,
        envelope: &NativeMessageEnvelope,
        provider_session_id: Option<&str>,
        provider_turn_id: Option<&str>,
    ) -> Result<Value, NativeProtocolError> {
        let text = render_envelope(envelope);
        match self {
            Self::ClaudeStreamJson => Ok(json!({
                "type": "user",
                "session_id": provider_session_id,
                "message": {"role": "user", "content": text},
                "wardian": {
                    "interaction_id": envelope.interaction_id,
                    "message_id": envelope.message_id,
                    "generation": envelope.generation
                }
            })),
            Self::AntigravityStreamJson => Ok(json!({
                "event": "user",
                "message": {"content": text}
            })),
            Self::CodexAppServer => {
                let thread_id = required_session_id(provider_session_id, self)?;
                let method = match envelope.operation {
                    NativeMessageOperation::StartTurn => "turn/start",
                    NativeMessageOperation::InvalidatePremise => "turn/steer",
                };
                let mut params = json!({
                    "threadId": thread_id,
                    "input": [{"type": "text", "text": text}],
                    "clientUserMessageId": envelope.message_id
                });
                if envelope.operation == NativeMessageOperation::InvalidatePremise {
                    params["expectedTurnId"] =
                        Value::String(required_turn_id(provider_turn_id, self)?.to_string());
                }
                Ok(json!({
                    "id": envelope.interaction_id,
                    "method": method,
                    "params": params
                }))
            }
            Self::OpenCodeAcp => {
                let session_id = required_session_id(provider_session_id, self)?;
                if envelope.operation == NativeMessageOperation::InvalidatePremise {
                    return Err(NativeProtocolError::UnsupportedOperation {
                        provider: self.provider().to_string(),
                        operation: "invalidate_premise".to_string(),
                    });
                }
                Ok(json!({
                    "jsonrpc": "2.0",
                    "id": envelope.interaction_id,
                    "method": "session/prompt",
                    "params": {
                        "sessionId": session_id,
                        "prompt": [{"type": "text", "text": text}]
                    }
                }))
            }
            Self::PiRpc => {
                let request_type = match envelope.operation {
                    NativeMessageOperation::StartTurn => "prompt",
                    NativeMessageOperation::InvalidatePremise => "steer",
                };
                Ok(json!({
                    "id": envelope.interaction_id,
                    "type": request_type,
                    "message": text
                }))
            }
        }
    }

    pub fn cancel_request(
        self,
        interaction_id: &str,
        provider_session_id: Option<&str>,
        provider_turn_id: Option<&str>,
    ) -> Result<Value, NativeProtocolError> {
        match self {
            Self::CodexAppServer => Ok(json!({
                "id": format!("cancel:{interaction_id}"),
                "method": "turn/interrupt",
                "params": {
                    "threadId": required_session_id(provider_session_id, self)?,
                    "turnId": required_turn_id(provider_turn_id, self)?
                }
            })),
            Self::OpenCodeAcp => Ok(json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": {"sessionId": required_session_id(provider_session_id, self)?}
            })),
            Self::PiRpc => Ok(json!({"id": format!("cancel:{interaction_id}"), "type": "abort"})),
            Self::ClaudeStreamJson => Ok(json!({
                "type": "control_request",
                "request_id": format!("cancel:{interaction_id}"),
                "request": {"subtype": "interrupt"}
            })),
            Self::AntigravityStreamJson => Err(NativeProtocolError::UnsupportedOperation {
                provider: self.provider().to_string(),
                operation: "cancel".to_string(),
            }),
        }
    }

    pub fn parse_line(self, line: &str) -> Result<Vec<NativeProtocolEvent>, NativeProtocolError> {
        let value: Value =
            serde_json::from_str(line).map_err(|error| NativeProtocolError::InvalidMessage {
                provider: self.provider().to_string(),
                message: error.to_string(),
            })?;
        Ok(match self {
            Self::ClaudeStreamJson => parse_claude_event(&value),
            Self::CodexAppServer => parse_codex_event(&value),
            Self::AntigravityStreamJson => parse_antigravity_event(&value),
            Self::OpenCodeAcp => parse_opencode_event(&value),
            Self::PiRpc => parse_pi_event(&value),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeProtocolEvent {
    pub kind: NativeProtocolEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl NativeProtocolEvent {
    pub fn delivery_phase(&self) -> Option<NativeDeliveryPhase> {
        match self.kind {
            NativeProtocolEventKind::ProviderAccepted => {
                Some(NativeDeliveryPhase::ProviderAccepted)
            }
            NativeProtocolEventKind::TurnStarted => Some(NativeDeliveryPhase::TurnStarted),
            NativeProtocolEventKind::TurnCompleted => Some(NativeDeliveryPhase::Completed),
            NativeProtocolEventKind::TurnFailed | NativeProtocolEventKind::ProtocolError => {
                Some(NativeDeliveryPhase::Failed)
            }
            NativeProtocolEventKind::TurnCancelled => Some(NativeDeliveryPhase::Cancelled),
            NativeProtocolEventKind::SessionBound
            | NativeProtocolEventKind::Progress
            | NativeProtocolEventKind::ApprovalRequested => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeProtocolEventKind {
    SessionBound,
    ProviderAccepted,
    TurnStarted,
    Progress,
    ApprovalRequested,
    TurnCompleted,
    TurnFailed,
    TurnCancelled,
    ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeProtocolError {
    UnsupportedOperation { provider: String, operation: String },
    MissingProviderSession { provider: String },
    MissingProviderTurn { provider: String },
    InvalidMessage { provider: String, message: String },
}

impl std::fmt::Display for NativeProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedOperation {
                provider,
                operation,
            } => {
                write!(
                    formatter,
                    "{provider} native transport does not support {operation}"
                )
            }
            Self::MissingProviderSession { provider } => {
                write!(
                    formatter,
                    "{provider} native transport has no bound provider session"
                )
            }
            Self::MissingProviderTurn { provider } => {
                write!(
                    formatter,
                    "{provider} native transport has no active provider turn"
                )
            }
            Self::InvalidMessage { provider, message } => {
                write!(
                    formatter,
                    "invalid {provider} native protocol message: {message}"
                )
            }
        }
    }
}

impl std::error::Error for NativeProtocolError {}

fn required_session_id(
    provider_session_id: Option<&str>,
    protocol: NativeProviderProtocol,
) -> Result<&str, NativeProtocolError> {
    provider_session_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| NativeProtocolError::MissingProviderSession {
            provider: protocol.provider().to_string(),
        })
}

fn required_turn_id(
    provider_turn_id: Option<&str>,
    protocol: NativeProviderProtocol,
) -> Result<&str, NativeProtocolError> {
    provider_turn_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| NativeProtocolError::MissingProviderTurn {
            provider: protocol.provider().to_string(),
        })
}

fn render_envelope(envelope: &NativeMessageEnvelope) -> String {
    let mut header = format!(
        "[Wardian message_id={} interaction_id={} generation={} target={}]",
        envelope.message_id, envelope.interaction_id, envelope.generation, envelope.target_agent_id
    );
    if let Some(sender) = envelope.sender_agent_id.as_deref() {
        header.push_str(&format!(" sender={sender}"));
    }
    if let Some(parent) = envelope.parent_interaction_id.as_deref() {
        header.push_str(&format!(" reply_to={parent}"));
    }
    if let Some(deadline) = envelope.deadline_at.as_deref() {
        header.push_str(&format!(" deadline={deadline}"));
    }
    format!("{header}\n{}", envelope.body)
}

fn parse_claude_event(value: &Value) -> Vec<NativeProtocolEvent> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let request_id = wardian_interaction_id(value);
    match kind {
        "system" if value.get("subtype").and_then(Value::as_str) == Some("init") => vec![event(
            NativeProtocolEventKind::SessionBound,
            request_id,
            string_at(value, &["session_id"]),
            None,
        )],
        "system" if value.get("subtype").and_then(Value::as_str) == Some("permission_request") => {
            vec![event(
                NativeProtocolEventKind::ApprovalRequested,
                request_id,
                None,
                string_at(value, &["tool_name"]),
            )]
        }
        "control_request"
            if value.pointer("/request/subtype").and_then(Value::as_str)
                == Some("can_use_tool") =>
        {
            vec![event(
                NativeProtocolEventKind::ApprovalRequested,
                string_at(value, &["request_id"]),
                None,
                string_at(value, &["request", "tool_name"]),
            )]
        }
        "control_response" => vec![event(
            if value.pointer("/response/subtype").and_then(Value::as_str) == Some("success") {
                NativeProtocolEventKind::ProviderAccepted
            } else {
                NativeProtocolEventKind::ProtocolError
            },
            string_at(value, &["response", "request_id"]),
            None,
            string_at(value, &["response", "error"]),
        )],
        "user" => vec![event(
            NativeProtocolEventKind::ProviderAccepted,
            request_id,
            string_at(value, &["session_id"]),
            None,
        )],
        "assistant" => vec![event(
            NativeProtocolEventKind::TurnStarted,
            request_id,
            string_at(value, &["session_id"]),
            assistant_text(value),
        )],
        "progress" | "message_stream" => vec![event(
            NativeProtocolEventKind::Progress,
            request_id,
            None,
            assistant_text(value),
        )],
        "result" => vec![event(
            if matches!(
                string_at(value, &["subtype"])
                    .or_else(|| string_at(value, &["stop_reason"]))
                    .as_deref(),
                Some("interrupted" | "cancelled" | "canceled")
            ) {
                NativeProtocolEventKind::TurnCancelled
            } else if value.get("is_error").and_then(Value::as_bool) == Some(true) {
                NativeProtocolEventKind::TurnFailed
            } else {
                NativeProtocolEventKind::TurnCompleted
            },
            request_id,
            string_at(value, &["session_id"]),
            string_at(value, &["result"]),
        )],
        _ => Vec::new(),
    }
}

fn parse_antigravity_event(value: &Value) -> Vec<NativeProtocolEvent> {
    let kind = value
        .get("event")
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let request_id = wardian_interaction_id(value);
    match kind {
        "init" | "system" => vec![event(
            NativeProtocolEventKind::SessionBound,
            request_id,
            string_at(value, &["conversation_id"])
                .or_else(|| string_at(value, &["init", "conversation_id"]))
                .or_else(|| string_at(value, &["session_id"])),
            None,
        )],
        "step_update" => {
            let step_type = string_at(value, &["step_update", "step_type"])
                .or_else(|| string_at(value, &["step_type"]));
            let state = string_at(value, &["step_update", "state"])
                .or_else(|| string_at(value, &["state"]));
            let kind =
                if step_type.as_deref() == Some("user_input") && state.as_deref() == Some("DONE") {
                    NativeProtocolEventKind::TurnStarted
                } else {
                    NativeProtocolEventKind::Progress
                };
            vec![event(
                kind,
                request_id,
                string_at(value, &["step_update", "conversation_id"]),
                string_at(value, &["step_update", "text_delta"])
                    .or_else(|| string_at(value, &["content"]))
                    .or_else(|| string_at(value, &["text"])),
            )]
        }
        "result" => vec![event(
            match string_at(value, &["result", "status"])
                .or_else(|| string_at(value, &["status"]))
                .as_deref()
            {
                Some("CANCELED" | "INTERRUPTED") => NativeProtocolEventKind::TurnCancelled,
                Some("ERROR" | "INVALID") => NativeProtocolEventKind::TurnFailed,
                _ if value.get("error").is_some_and(|error| !error.is_null()) => {
                    NativeProtocolEventKind::TurnFailed
                }
                _ => NativeProtocolEventKind::TurnCompleted,
            },
            request_id,
            string_at(value, &["result", "conversation_id"])
                .or_else(|| string_at(value, &["conversation_id"])),
            string_at(value, &["result", "response"]).or_else(|| string_at(value, &["content"])),
        )],
        _ => Vec::new(),
    }
}

fn parse_pi_event(value: &Value) -> Vec<NativeProtocolEvent> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let request_id = string_at(value, &["id"]);
    match kind {
        "response" if value.get("command").and_then(Value::as_str) == Some("get_state") => {
            vec![event(
                NativeProtocolEventKind::SessionBound,
                request_id,
                string_at(value, &["data", "sessionId"]),
                None,
            )]
        }
        "response"
            if matches!(
                value.get("command").and_then(Value::as_str),
                Some("prompt" | "steer" | "follow_up")
            ) =>
        {
            vec![event(
                if value.get("success").and_then(Value::as_bool) == Some(true) {
                    NativeProtocolEventKind::ProviderAccepted
                } else {
                    NativeProtocolEventKind::TurnFailed
                },
                request_id,
                None,
                string_at(value, &["error"]),
            )]
        }
        "agent_start" | "turn_start" => vec![event(
            NativeProtocolEventKind::TurnStarted,
            request_id,
            None,
            None,
        )],
        "message_start"
        | "message_update"
        | "tool_execution_start"
        | "tool_execution_update"
        | "tool_execution_end" => vec![event(
            NativeProtocolEventKind::Progress,
            request_id,
            None,
            assistant_text(value),
        )],
        "extension_ui_request" => vec![event(
            NativeProtocolEventKind::ApprovalRequested,
            request_id,
            None,
            string_at(value, &["title"]).or_else(|| string_at(value, &["message"])),
        )],
        "agent_settled" => vec![event(
            NativeProtocolEventKind::TurnCompleted,
            request_id,
            None,
            None,
        )],
        "agent_end" if value.get("willRetry").and_then(Value::as_bool) == Some(false) => {
            vec![event(
                NativeProtocolEventKind::Progress,
                request_id,
                None,
                None,
            )]
        }
        _ => Vec::new(),
    }
}

fn parse_codex_event(value: &Value) -> Vec<NativeProtocolEvent> {
    if let Some(error) = value.get("error") {
        return vec![NativeProtocolEvent {
            kind: NativeProtocolEventKind::ProtocolError,
            request_id: json_rpc_id(value),
            provider_session_id: None,
            provider_turn_id: None,
            text: None,
            detail: Some(error.to_string()),
        }];
    }
    if value.get("result").is_some() {
        let request_id = json_rpc_id(value);
        if request_id
            .as_deref()
            .is_some_and(|id| id.starts_with("wardian:init:"))
        {
            return Vec::new();
        }
        let provider_session_id = string_at(value, &["result", "thread", "id"])
            .or_else(|| string_at(value, &["result", "threadId"]));
        let provider_turn_id = string_at(value, &["result", "turn", "id"])
            .or_else(|| string_at(value, &["result", "turnId"]));
        let kind = if request_id
            .as_deref()
            .is_some_and(|id| id.starts_with("wardian:thread:"))
        {
            NativeProtocolEventKind::SessionBound
        } else if provider_turn_id.is_some() {
            NativeProtocolEventKind::ProviderAccepted
        } else {
            return Vec::new();
        };
        return vec![NativeProtocolEvent {
            kind,
            request_id,
            provider_session_id,
            provider_turn_id,
            text: None,
            detail: None,
        }];
    }

    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = value.get("params").unwrap_or(&Value::Null);
    match method {
        "thread/started" => vec![event(
            NativeProtocolEventKind::SessionBound,
            None,
            string_at(params, &["thread", "id"]).or_else(|| string_at(params, &["threadId"])),
            None,
        )],
        "turn/started" => vec![NativeProtocolEvent {
            kind: NativeProtocolEventKind::TurnStarted,
            request_id: string_at(params, &["turn", "clientUserMessageId"])
                .or_else(|| string_at(params, &["clientUserMessageId"])),
            provider_session_id: string_at(params, &["threadId"]),
            provider_turn_id: string_at(params, &["turn", "id"])
                .or_else(|| string_at(params, &["turnId"])),
            text: None,
            detail: None,
        }],
        "turn/completed" => vec![NativeProtocolEvent {
            kind: match string_at(params, &["turn", "status"]).as_deref() {
                Some("failed") => NativeProtocolEventKind::TurnFailed,
                Some("interrupted" | "cancelled" | "canceled") => {
                    NativeProtocolEventKind::TurnCancelled
                }
                _ => NativeProtocolEventKind::TurnCompleted,
            },
            request_id: string_at(params, &["turn", "clientUserMessageId"]),
            provider_session_id: string_at(params, &["threadId"]),
            provider_turn_id: string_at(params, &["turn", "id"]),
            text: None,
            detail: string_at(params, &["turn", "error", "message"]),
        }],
        "item/started" | "item/completed" | "item/agentMessage/delta" => vec![event(
            NativeProtocolEventKind::Progress,
            string_at(params, &["turnId"]),
            None,
            string_at(params, &["delta"]),
        )],
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => vec![event(
            NativeProtocolEventKind::ApprovalRequested,
            json_rpc_id(value),
            None,
            Some(method.to_string()),
        )],
        _ => Vec::new(),
    }
}

fn parse_opencode_event(value: &Value) -> Vec<NativeProtocolEvent> {
    if let Some(error) = value.get("error") {
        return vec![NativeProtocolEvent {
            kind: NativeProtocolEventKind::ProtocolError,
            request_id: json_rpc_id(value),
            provider_session_id: None,
            provider_turn_id: None,
            text: None,
            detail: Some(error.to_string()),
        }];
    }
    if value.get("result").is_some() {
        let request_id = json_rpc_id(value);
        if request_id
            .as_deref()
            .is_some_and(|id| id.starts_with("wardian:init:"))
        {
            return Vec::new();
        }
        if request_id
            .as_deref()
            .is_some_and(|id| id.starts_with("wardian:session:"))
        {
            return vec![event(
                NativeProtocolEventKind::SessionBound,
                request_id,
                string_at(value, &["result", "sessionId"]),
                None,
            )];
        }
        let stop_reason = string_at(value, &["result", "stopReason"]);
        return vec![NativeProtocolEvent {
            kind: if stop_reason.as_deref() == Some("cancelled") {
                NativeProtocolEventKind::TurnCancelled
            } else {
                NativeProtocolEventKind::TurnCompleted
            },
            request_id,
            provider_session_id: None,
            provider_turn_id: None,
            text: None,
            detail: if stop_reason.as_deref() == Some("end_turn") {
                None
            } else {
                stop_reason
            },
        }];
    }
    match value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "session/update" => vec![event(
            NativeProtocolEventKind::TurnStarted,
            None,
            string_at(value, &["params", "sessionId"]),
            string_at(value, &["params", "update", "content", "text"])
                .or_else(|| string_at(value, &["params", "update", "content"]))
                .or_else(|| string_at(value, &["params", "update", "text"])),
        )],
        "session/request_permission" => vec![event(
            NativeProtocolEventKind::ApprovalRequested,
            json_rpc_id(value),
            string_at(value, &["params", "sessionId"]),
            string_at(value, &["params", "title"]),
        )],
        _ => Vec::new(),
    }
}

fn event(
    kind: NativeProtocolEventKind,
    request_id: Option<String>,
    provider_session_id: Option<String>,
    text: Option<String>,
) -> NativeProtocolEvent {
    NativeProtocolEvent {
        kind,
        request_id,
        provider_session_id,
        provider_turn_id: None,
        text,
        detail: None,
    }
}

fn json_rpc_id(value: &Value) -> Option<String> {
    value.get("id").and_then(|id| match id {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn wardian_interaction_id(value: &Value) -> Option<String> {
    string_at(value, &["wardian", "interaction_id"])
        .or_else(|| assistant_text(value).and_then(|text| marker_value(&text, "interaction_id")))
}

fn marker_value(text: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=");
    let start = text.find(&marker)? + marker.len();
    let tail = &text[start..];
    let end = tail
        .find(|ch: char| ch.is_whitespace() || ch == ']')
        .unwrap_or(tail.len());
    let value = tail[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn assistant_text(value: &Value) -> Option<String> {
    string_at(value, &["message", "content"])
        .or_else(|| string_at(value, &["message", "text"]))
        .or_else(|| string_at(value, &["content"]))
        .or_else(|| string_at(value, &["text"]))
        .or_else(|| string_at(value, &["assistantMessageEvent", "delta"]))
}

fn string_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(operation: NativeMessageOperation) -> NativeMessageEnvelope {
        NativeMessageEnvelope {
            interaction_id: "ask_1".into(),
            message_id: "msg_1".into(),
            target_agent_id: "agent-1".into(),
            sender_agent_id: Some("orchestrator".into()),
            parent_interaction_id: None,
            caller_idempotency_key: Some("caller-1".into()),
            generation: 7,
            operation,
            deadline_at: None,
            body: "Review this patch".into(),
        }
    }

    #[test]
    fn all_maintained_non_gemini_providers_have_native_protocols() {
        for provider in ["claude", "codex", "antigravity", "opencode", "pi"] {
            let protocol = NativeProviderProtocol::for_provider(provider).expect(provider);
            assert!(protocol.capabilities("test").persistent_session);
            assert!(protocol.capabilities("test").positive_turn_start);
        }
        assert!(NativeProviderProtocol::for_provider("gemini").is_none());
        assert!(NativeProviderProtocol::for_provider("mock").is_none());
    }

    #[test]
    fn codex_uses_turn_start_and_client_message_identity() {
        let request = NativeProviderProtocol::CodexAppServer
            .submit_request(
                &envelope(NativeMessageOperation::StartTurn),
                Some("thread-1"),
                None,
            )
            .expect("codex request");

        assert_eq!(request["method"], "turn/start");
        assert_eq!(request["params"]["threadId"], "thread-1");
        assert_eq!(request["params"]["clientUserMessageId"], "msg_1");
    }

    #[test]
    fn codex_steering_and_interrupt_are_fenced_to_the_active_turn() {
        let steer = NativeProviderProtocol::CodexAppServer
            .submit_request(
                &envelope(NativeMessageOperation::InvalidatePremise),
                Some("thread-1"),
                Some("turn-1"),
            )
            .expect("fenced Codex steer");
        let interrupt = NativeProviderProtocol::CodexAppServer
            .cancel_request("ask_1", Some("thread-1"), Some("turn-1"))
            .expect("fenced Codex interrupt");

        assert_eq!(steer["method"], "turn/steer");
        assert_eq!(steer["params"]["expectedTurnId"], "turn-1");
        assert_eq!(interrupt["method"], "turn/interrupt");
        assert_eq!(interrupt["params"]["turnId"], "turn-1");
        assert!(matches!(
            NativeProviderProtocol::CodexAppServer.submit_request(
                &envelope(NativeMessageOperation::InvalidatePremise),
                Some("thread-1"),
                None,
            ),
            Err(NativeProtocolError::MissingProviderTurn { .. })
        ));
    }

    #[test]
    fn broad_mid_turn_injection_is_not_available_on_opencode() {
        let result = NativeProviderProtocol::OpenCodeAcp.submit_request(
            &envelope(NativeMessageOperation::InvalidatePremise),
            Some("session-1"),
            None,
        );
        assert!(matches!(
            result,
            Err(NativeProtocolError::UnsupportedOperation { .. })
        ));
    }

    #[test]
    fn pi_prompt_ack_and_agent_start_are_distinct_phases() {
        let accepted = NativeProviderProtocol::PiRpc
            .parse_line(r#"{"id":"ask_1","type":"response","command":"prompt","success":true}"#)
            .expect("pi response");
        let started = NativeProviderProtocol::PiRpc
            .parse_line(r#"{"type":"agent_start"}"#)
            .expect("pi event");

        assert_eq!(accepted[0].kind, NativeProtocolEventKind::ProviderAccepted);
        assert_eq!(started[0].kind, NativeProtocolEventKind::TurnStarted);
    }

    #[test]
    fn claude_replay_acceptance_and_assistant_start_are_distinct() {
        let accepted = NativeProviderProtocol::ClaudeStreamJson
            .parse_line(
                r#"{"type":"user","session_id":"claude-1","message":{"role":"user","content":"[Wardian message_id=msg_1 interaction_id=ask_1 generation=7 target=agent-1] Review"}}"#,
            )
            .expect("claude event");
        let started = NativeProviderProtocol::ClaudeStreamJson
            .parse_line(
                r#"{"type":"assistant","session_id":"claude-1","message":{"role":"assistant","content":[{"type":"text","text":"Working"}]},"wardian":{"interaction_id":"ask_1"}}"#,
            )
            .expect("claude assistant event");

        assert_eq!(accepted[0].kind, NativeProtocolEventKind::ProviderAccepted);
        assert_eq!(accepted[0].request_id.as_deref(), Some("ask_1"));
        assert_eq!(started[0].kind, NativeProtocolEventKind::TurnStarted);
    }

    #[test]
    fn claude_control_ack_is_distinct_from_interrupted_terminal_result() {
        let acknowledged = NativeProviderProtocol::ClaudeStreamJson
            .parse_line(
                r#"{"type":"control_response","response":{"subtype":"success","request_id":"cancel:ask_1","response":{}}}"#,
            )
            .expect("Claude control response");
        let terminal = NativeProviderProtocol::ClaudeStreamJson
            .parse_line(
                r#"{"type":"result","subtype":"interrupted","is_error":true,"session_id":"claude-1"}"#,
            )
            .expect("Claude interrupted result");

        assert_eq!(
            acknowledged[0].kind,
            NativeProtocolEventKind::ProviderAccepted
        );
        assert_eq!(terminal[0].kind, NativeProtocolEventKind::TurnCancelled);
    }

    #[test]
    fn antigravity_intermediate_step_is_not_completion() {
        let events = NativeProviderProtocol::AntigravityStreamJson
            .parse_line(r#"{"event":"step_update","step_update":{"step_type":"planner","state":"DONE","text_delta":"planning"}}"#)
            .expect("antigravity event");

        assert_eq!(events[0].kind, NativeProtocolEventKind::Progress);
    }

    #[test]
    fn antigravity_user_input_done_is_positive_turn_start() {
        let events = NativeProviderProtocol::AntigravityStreamJson
            .parse_line(r#"{"event":"step_update","step_update":{"conversation_id":"conv-1","step_type":"user_input","state":"DONE"}}"#)
            .expect("antigravity event");

        assert_eq!(events[0].kind, NativeProtocolEventKind::TurnStarted);
        assert_eq!(events[0].provider_session_id.as_deref(), Some("conv-1"));
    }

    #[test]
    fn codex_turn_started_uses_provider_notification() {
        let events = NativeProviderProtocol::CodexAppServer
            .parse_line(
                r#"{"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","clientUserMessageId":"msg_1"}}}"#,
            )
            .expect("codex event");

        assert_eq!(events[0].kind, NativeProtocolEventKind::TurnStarted);
        assert_eq!(events[0].request_id.as_deref(), Some("msg_1"));
        assert_eq!(events[0].provider_turn_id.as_deref(), Some("turn-1"));
    }

    #[test]
    fn acp_first_session_update_proves_turn_start() {
        let events = NativeProviderProtocol::OpenCodeAcp
            .parse_line(
                r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"ses_1","update":{"sessionUpdate":"agent_message_chunk"}}}"#,
            )
            .expect("acp event");

        assert_eq!(events[0].kind, NativeProtocolEventKind::TurnStarted);
        assert_eq!(events[0].provider_session_id.as_deref(), Some("ses_1"));
    }

    #[test]
    fn acp_agent_message_chunk_preserves_response_text() {
        let events = NativeProviderProtocol::OpenCodeAcp
            .parse_line(
                r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"ses_1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"WARDIAN_NATIVE_OPENCODE_OK"}}}}"#,
            )
            .expect("ACP agent message chunk");

        assert_eq!(events[0].kind, NativeProtocolEventKind::TurnStarted);
        assert_eq!(
            events[0].text.as_deref(),
            Some("WARDIAN_NATIVE_OPENCODE_OK")
        );
    }
}

use crate::remote::models::{
    RemoteAgentActionRequest, RemoteAgentSummary, RemoteInboxActionRequest, RemoteTerminalSnapshot,
    RemoteWatchlistResponse,
};
use crate::state::AppState;
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Manager};
use wardian_core::control::{InboxNotificationKind, InteractionStatus, MessageInputMode};
use wardian_core::models::chat::AgentChatEvent;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RemoteAgentChatPage {
    pub events: Vec<AgentChatEvent>,
    pub has_older: bool,
    pub next_before: Option<usize>,
}

pub async fn remote_agent_roster(state: &AppState) -> Vec<RemoteAgentSummary> {
    let agents = state.agents.lock().await;
    let order = state.agent_order.lock().await;
    let mut summaries_by_id = agents
        .values()
        .filter_map(|agent| {
            let config = agent.config.lock().ok()?.clone();
            let status = agent.current_status.lock().ok()?.clone();

            Some((
                config.session_id.clone(),
                RemoteAgentSummary {
                    session_id: config.session_id,
                    session_name: config.session_name,
                    agent_class: config.agent_class,
                    provider: config.provider,
                    workspace: config.folder,
                    status,
                    latest_text: None,
                },
            ))
        })
        .collect::<HashMap<_, _>>();

    let mut ordered = Vec::with_capacity(summaries_by_id.len());
    for session_id in order.iter() {
        if let Some(summary) = summaries_by_id.remove(session_id) {
            ordered.push(summary);
        }
    }

    let mut remaining = summaries_by_id.into_values().collect::<Vec<_>>();
    remaining.sort_by(|left, right| {
        left.session_name
            .cmp(&right.session_name)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    ordered.extend(remaining);
    ordered
}

pub fn remote_watchlist_state() -> Result<RemoteWatchlistResponse, String> {
    let Some(home) = crate::utils::fs::get_wardian_home() else {
        return Ok(RemoteWatchlistResponse {
            watchlists: serde_json::json!([]),
            teams: serde_json::json!([]),
            prefs: None,
        });
    };

    let persisted_state = std::fs::read_to_string(home.join("watchlists").join("index.json"))
        .ok()
        .and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok())
        .unwrap_or_else(|| serde_json::json!([]));
    let (watchlists, teams) = if let Some(state) = persisted_state.as_object() {
        (
            state
                .get("watchlists")
                .filter(|value| value.is_array())
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
            state
                .get("teams")
                .filter(|value| value.is_array())
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
    } else if persisted_state.is_array() {
        (persisted_state, serde_json::json!([]))
    } else {
        (serde_json::json!([]), serde_json::json!([]))
    };
    let prefs = std::fs::read_to_string(home.join("watchlists").join("prefs.json"))
        .ok()
        .and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok());

    Ok(RemoteWatchlistResponse {
        watchlists,
        teams,
        prefs,
    })
}

/// Builds the same Inbox projection as the desktop queue store.
pub async fn remote_queue_items(state: &AppState) -> Vec<serde_json::Value> {
    let persisted_items = crate::utils::fs::get_wardian_home()
        .and_then(|home| std::fs::read_to_string(home.join("queue").join("items.json")).ok())
        .and_then(|data| serde_json::from_str::<Vec<serde_json::Value>>(&data).ok())
        .unwrap_or_default();
    let read_notification_ids = persisted_items
        .iter()
        .filter(|item| {
            item.get("type").and_then(serde_json::Value::as_str) == Some("agent_update")
                && item.get("read").and_then(serde_json::Value::as_bool) == Some(true)
        })
        .filter_map(|item| {
            item.get("inbox_notification_id")
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_string)
        .collect::<std::collections::HashSet<_>>();
    let legacy_items = persisted_items.into_iter().filter(|item| {
        item.get("inbox_notification_id").is_none()
            && item.get("automation_approval").is_none()
            && item.get("dismissed").is_none()
    });
    let notifications = crate::commands::inbox::list_inbox_notifications_for_state(state)
        .await
        .map(|result| result.notifications)
        .unwrap_or_default()
        .into_iter()
        .map(|notification| {
            let is_approval = matches!(&notification.kind, InboxNotificationKind::Approval);
            serde_json::json!({
                "id": format!("notification:{}", notification.id),
                "type": if is_approval { "approval_request" } else { "agent_update" },
                "timestamp": queue_timestamp(&notification.created_at),
                "read": if is_approval { notification.status != InteractionStatus::AwaitingReply } else { read_notification_ids.contains(notification.id.as_str()) },
                "agent_session_id": notification.sender_session_id,
                "notification_title": notification.title,
                "inbox_notification_id": notification.id,
                "notification_status": notification.status,
                "summary": notification.body,
                "proposed_action": notification.proposed_action,
                "risk": notification.risk,
                "approval_choices": notification.choices,
                "approval_decision": notification.decision.map(|decision| decision.choice),
                "expires_at": notification.expires_at,
            })
        });
    let automation_approvals = crate::commands::inbox::list_automation_inbox_approvals()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|approval| serde_json::json!({
            "id": format!("automation-approval:{}:{}:{}", approval.blueprint_id, approval.run_id, approval.node),
            "type": "approval_request",
            "timestamp": approval.created_at.as_deref().map(queue_timestamp).unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
            "read": false,
            "automation_id": approval.blueprint_id,
            "automation_run_id": approval.run_id,
            "automation_name": approval.title,
            "notification_title": approval.title,
            "summary": approval.prompt,
            "proposed_action": "Continue this automation beyond its approval gate",
            "risk": "The automation will execute the next authored steps after approval.",
            "approval_choices": ["Approve", "Reject"],
            "automation_approval": { "blueprint_id": approval.blueprint_id, "blueprint_path": approval.blueprint_path, "run_id": approval.run_id, "node": approval.node },
        }));
    let mut items = notifications
        .chain(automation_approvals)
        .chain(legacy_items)
        .collect::<Vec<_>>();
    items.sort_by_key(|item| {
        std::cmp::Reverse(
            item.get("timestamp")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default(),
        )
    });
    items
}

fn persisted_queue_items() -> Vec<serde_json::Value> {
    crate::utils::queue::load_items()
}

fn save_persisted_queue_items(items: &[serde_json::Value]) -> Result<(), String> {
    crate::utils::queue::save_items(items)
}

fn is_legacy_queue_item(item: &serde_json::Value) -> bool {
    item.get("inbox_notification_id").is_none() && item.get("automation_approval").is_none()
}

fn is_clearable_legacy_completion(item: &serde_json::Value) -> bool {
    is_legacy_queue_item(item)
        && item.get("dismissed").is_none()
        && !provider_choice_acknowledgement_unresolved(item)
        && matches!(
            item.get("type").and_then(serde_json::Value::as_str),
            Some("agent_completed" | "automation_completed")
        )
}

fn is_pending_approval(item: &serde_json::Value) -> bool {
    item.get("automation_approval").is_some()
        || (item.get("type").and_then(serde_json::Value::as_str) == Some("approval_request")
            && item
                .get("notification_status")
                .and_then(serde_json::Value::as_str)
                == Some("awaiting_reply"))
}

fn provider_choice_acknowledgement_unresolved(item: &serde_json::Value) -> bool {
    item.get("provider_choice_pending").is_some()
        || (item.get("provider_choice_sent").is_some()
            && item.get("read").and_then(serde_json::Value::as_bool) != Some(true))
}

fn notification_read_acknowledgement(notification_id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": format!("notification-read:{notification_id}"),
        "type": "agent_update",
        "timestamp": chrono::Utc::now().timestamp_millis(),
        "read": true,
        "inbox_notification_id": notification_id,
    })
}

fn current_queue_item<'a>(
    items: &'a [serde_json::Value],
    item_id: &str,
) -> Result<&'a serde_json::Value, String> {
    items
        .iter()
        .find(|item| item.get("id").and_then(serde_json::Value::as_str) == Some(item_id))
        .ok_or_else(|| "inbox_item_not_found".to_string())
}

fn validate_remote_mark_read(item: &serde_json::Value) -> Result<(), String> {
    if is_pending_approval(item) {
        return Err("pending_approval_cannot_be_marked_read".to_string());
    }
    if item.get("provider_choice_pending").is_some() {
        return Err("provider_choice_delivery_uncertain_cannot_be_marked_read".to_string());
    }
    Ok(())
}

/// Applies a mobile Inbox mutation to the same persisted projection used by
/// the desktop Inbox. The caller must authenticate and rate-limit the request.
pub async fn apply_remote_inbox_action(
    state: &AppState,
    app: &AppHandle,
    request: RemoteInboxActionRequest,
) -> Result<(), String> {
    let _queue_guard = state.queue_io_lock.lock().await;
    let projected_items = remote_queue_items(state).await;
    match request.action.as_str() {
        "mark_read" => {
            let item_id = request
                .item_id
                .as_deref()
                .ok_or_else(|| "item_id_required".to_string())?;
            let item = current_queue_item(&projected_items, item_id)?;
            validate_remote_mark_read(item)?;
            let mut persisted = persisted_queue_items();
            if let Some(notification_id) = item
                .get("inbox_notification_id")
                .and_then(serde_json::Value::as_str)
            {
                if !persisted.iter().any(|candidate| {
                    candidate
                        .get("inbox_notification_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(notification_id)
                        && candidate.get("read").and_then(serde_json::Value::as_bool) == Some(true)
                }) {
                    persisted.push(notification_read_acknowledgement(notification_id));
                }
            } else if let Some(candidate) = persisted.iter_mut().find(|candidate| {
                candidate.get("id").and_then(serde_json::Value::as_str) == Some(item_id)
            }) {
                candidate["read"] = serde_json::Value::Bool(true);
            } else {
                return Err("inbox_item_not_persisted".to_string());
            }
            save_persisted_queue_items(&persisted)?;
        }
        "mark_all_read" => {
            let mut persisted = persisted_queue_items();
            for item in persisted.iter_mut().filter(|item| {
                is_legacy_queue_item(item) && !provider_choice_acknowledgement_unresolved(item)
            }) {
                item["read"] = serde_json::Value::Bool(true);
            }
            let known_acknowledgements = persisted
                .iter()
                .filter(|item| item.get("read").and_then(serde_json::Value::as_bool) == Some(true))
                .filter_map(|item| {
                    item.get("inbox_notification_id")
                        .and_then(serde_json::Value::as_str)
                })
                .map(str::to_string)
                .collect::<std::collections::HashSet<_>>();
            for item in projected_items.iter().filter(|item| {
                item.get("type").and_then(serde_json::Value::as_str) == Some("agent_update")
            }) {
                if let Some(notification_id) = item
                    .get("inbox_notification_id")
                    .and_then(serde_json::Value::as_str)
                {
                    if !known_acknowledgements.contains(notification_id) {
                        persisted.push(notification_read_acknowledgement(notification_id));
                    }
                }
            }
            save_persisted_queue_items(&persisted)?;
        }
        "clear_read" => {
            let persisted = persisted_queue_items();
            let next = persisted
                .into_iter()
                .filter(|item| {
                    !(is_clearable_legacy_completion(item)
                        && item.get("read").and_then(serde_json::Value::as_bool) == Some(true))
                })
                .collect::<Vec<_>>();
            save_persisted_queue_items(&next)?;
        }
        "dismiss" => {
            let item_id = request
                .item_id
                .as_deref()
                .ok_or_else(|| "item_id_required".to_string())?;
            let item = current_queue_item(&projected_items, item_id)?;
            if item.get("automation_approval").is_some()
                || item.get("inbox_notification_id").is_some()
                || provider_choice_acknowledgement_unresolved(item)
            {
                return Err("inbox_item_not_dismissible".to_string());
            }
            let persisted = persisted_queue_items();
            let next = persisted
                .into_iter()
                .filter(|candidate| {
                    candidate.get("id").and_then(serde_json::Value::as_str) != Some(item_id)
                })
                .collect::<Vec<_>>();
            save_persisted_queue_items(&next)?;
        }
        "resolve_approval" => {
            let item_id = request
                .item_id
                .as_deref()
                .ok_or_else(|| "item_id_required".to_string())?;
            let choice = request
                .choice
                .as_deref()
                .ok_or_else(|| "choice_required".to_string())?;
            let item = current_queue_item(&projected_items, item_id)?;
            let choices = item
                .get("approval_choices")
                .and_then(serde_json::Value::as_array)
                .map(|choices| {
                    choices
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !choices.contains(&choice) {
                return Err("invalid_choice".to_string());
            }
            if let Some(notification_id) = item
                .get("inbox_notification_id")
                .and_then(serde_json::Value::as_str)
            {
                state
                    .interactions
                    .resolve_notification(notification_id, choice)
                    .await
                    .map_err(|error| error.to_string())?;
            } else if let Some(automation_approval) = item.get("automation_approval") {
                crate::commands::automation::approve_automation_for_surface(
                    state,
                    app.clone(),
                    automation_approval
                        .get("blueprint_id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| "automation_blueprint_id_missing".to_string())?
                        .to_string(),
                    automation_approval
                        .get("run_id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| "automation_run_id_missing".to_string())?
                        .to_string(),
                    automation_approval
                        .get("blueprint_path")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| "automation_blueprint_path_missing".to_string())?
                        .to_string(),
                    automation_approval
                        .get("node")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| "automation_node_missing".to_string())?
                        .to_string(),
                    choice == "Approve",
                    "user".to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .await?;
            } else {
                return Err("inbox_item_not_approval".to_string());
            }
        }
        _ => return Err("unsupported_inbox_action".to_string()),
    }
    let _ = app.emit("inbox-updated", ());
    Ok(())
}

fn queue_timestamp(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.timestamp_millis())
        .unwrap_or_else(|_| chrono::Utc::now().timestamp_millis())
}

pub async fn remote_agent_chat_transcript(
    state: &AppState,
    session_id: &str,
) -> Result<Vec<AgentChatEvent>, String> {
    crate::commands::chat::load_agent_chat_transcript_for_state(state, session_id.to_string()).await
}

pub async fn remote_agent_chat_page(
    state: &AppState,
    session_id: &str,
    before: Option<usize>,
    limit: usize,
) -> Result<RemoteAgentChatPage, String> {
    let events = remote_agent_chat_transcript(state, session_id).await?;
    Ok(page_remote_agent_chat_events(events, before, limit))
}

pub fn page_remote_agent_chat_events(
    events: Vec<AgentChatEvent>,
    before: Option<usize>,
    limit: usize,
) -> RemoteAgentChatPage {
    let end = before.unwrap_or(events.len()).min(events.len());
    let start = end.saturating_sub(limit.max(1));

    RemoteAgentChatPage {
        events: events[start..end].to_vec(),
        has_older: start > 0,
        next_before: (start > 0).then_some(start),
    }
}

pub async fn remote_agent_terminal_snapshot(
    state: &AppState,
    session_id: &str,
    since: Option<&str>,
    tail_bytes: Option<usize>,
) -> Result<RemoteTerminalSnapshot, String> {
    let watch_state = {
        let agents = state.agents.lock().await;
        agents
            .get(session_id)
            .map(|agent| agent.watch_state.clone())
            .ok_or_else(|| "agent_not_found".to_string())?
    };
    let snapshot = watch_state
        .lock()
        .map_err(|_| "watch_state_unavailable".to_string())?
        .snapshot_since(since, tail_bytes)
        .map_err(|error| error.code().to_string())?;

    Ok(RemoteTerminalSnapshot {
        cursor: snapshot.output.cursor,
        text: snapshot.output.text,
        truncated: snapshot.output.truncated,
        omitted_bytes: snapshot.output.omitted_bytes,
    })
}

pub async fn remote_agent_terminal_raw_output(
    state: &AppState,
    session_id: &str,
    tail_bytes: Option<usize>,
) -> Result<String, String> {
    let watch_state = {
        let agents = state.agents.lock().await;
        agents
            .get(session_id)
            .map(|agent| agent.watch_state.clone())
            .ok_or_else(|| "agent_not_found".to_string())?
    };
    let snapshot = watch_state
        .lock()
        .map_err(|_| "watch_state_unavailable".to_string())?
        .raw_snapshot_since(None, tail_bytes)
        .map_err(|error| error.code().to_string())?;

    Ok(snapshot.text)
}

pub fn validate_remote_agent_action(request: &RemoteAgentActionRequest) -> Result<(), String> {
    if request.action == "send_prompt" {
        if request
            .prompt
            .as_ref()
            .is_none_or(|prompt| prompt.trim().is_empty())
        {
            Err("prompt_required".to_string())
        } else if matches!(
            request.input_mode.unwrap_or_default(),
            MessageInputMode::Message | MessageInputMode::Command
        ) {
            Ok(())
        } else {
            Err("unsupported_remote_input_mode".to_string())
        }
    } else {
        match request.action.as_str() {
            "pause" | "resume" | "clear" | "kill" => Ok(()),
            _ => Err("unsupported_remote_agent_action".to_string()),
        }
    }
}

pub async fn run_remote_agent_action(
    app: &AppHandle,
    request: RemoteAgentActionRequest,
) -> Result<(), String> {
    validate_remote_agent_action(&request)?;
    match request.action.as_str() {
        "send_prompt" => {
            let state = app.state::<AppState>();
            let prompt = request.prompt.unwrap_or_default();
            let _queue_guard = if request.inbox_item_id.is_some() {
                Some(state.queue_io_lock.lock().await)
            } else {
                None
            };
            send_remote_prompt_with_idempotency(
                app,
                &state,
                &request.target,
                &prompt,
                request.input_mode.unwrap_or_default(),
                request.inbox_item_id.as_deref(),
            )
            .await
        }
        "pause" => {
            let state = app.state::<AppState>();
            crate::commands::agent::pause_agent(request.target, state, app.clone()).await
        }
        "resume" => {
            let state = app.state::<AppState>();
            crate::commands::agent::resume_agent(request.target, state, app.clone()).await
        }
        "clear" => {
            let state = app.state::<AppState>();
            crate::commands::agent::clear_agent_session(request.target, None, state, app.clone())
                .await
        }
        "kill" => {
            let state = app.state::<AppState>();
            crate::commands::agent::kill_agent(request.target, state, app.clone()).await
        }
        _ => Err("unsupported_remote_agent_action".to_string()),
    }
}

async fn send_remote_prompt_with_idempotency(
    app: &AppHandle,
    state: &AppState,
    target: &str,
    prompt: &str,
    input_mode: MessageInputMode,
    inbox_item_id: Option<&str>,
) -> Result<(), String> {
    if let Some(item_id) = inbox_item_id {
        let mut persisted = crate::utils::queue::load_items();
        let index = persisted
            .iter()
            .position(|item| item.get("id").and_then(serde_json::Value::as_str) == Some(item_id))
            .ok_or_else(|| "inbox_item_not_found".to_string())?;
        let item = &persisted[index];
        if item
            .get("agent_session_id")
            .and_then(serde_json::Value::as_str)
            != Some(target)
        {
            return Err("inbox_item_agent_mismatch".to_string());
        }
        if item.get("type").and_then(serde_json::Value::as_str) != Some("action_needed") {
            return Err("inbox_item_not_provider_action".to_string());
        }
        if let Some(pending_choice) = item
            .get("provider_choice_pending")
            .and_then(serde_json::Value::as_str)
        {
            return if pending_choice == prompt {
                Err("provider_choice_delivery_uncertain".to_string())
            } else {
                Err("provider_choice_already_sent".to_string())
            };
        }
        if let Some(sent_choice) = item
            .get("provider_choice_sent")
            .and_then(serde_json::Value::as_str)
        {
            return if sent_choice == prompt {
                Ok(())
            } else {
                Err("provider_choice_already_sent".to_string())
            };
        }
        // Record the choice before touching the provider. A restart between this
        // write and the native dispatch must recover as explicitly uncertain,
        // never as an absent choice that can be replayed blindly.
        persisted[index]["provider_choice_pending"] = serde_json::Value::String(prompt.to_string());
        crate::utils::queue::save_items(&persisted)?;
        let result = crate::delivery::submit_live_surface_prompt(
            Some(app),
            state,
            crate::delivery::LiveSurfacePromptRequest {
                session_id: target.to_string(),
                prompt: prompt.to_string(),
                interaction_id: None,
                input_mode,
                queue_policy: wardian_core::control::QueuePolicy::LiveOnly,
                approval_action: None,
                origin: None,
                runtime_state: "live_pty_available",
                mark_prompt_started: true,
                require_provider_turn_receipt: true,
                payload_sent_detail: None,
                delivery_message_id: None,
            },
        )
        .await;
        return match result {
            Ok(_) => {
                persisted[index]["provider_choice_sent"] =
                    serde_json::Value::String(prompt.to_string());
                persisted[index]
                    .as_object_mut()
                    .expect("queue item object")
                    .remove("provider_choice_pending");
                crate::utils::queue::save_items(&persisted)?;
                Ok(())
            }
            Err(error) => {
                if error.retry_safe {
                    if let Some(item) = persisted.get_mut(index) {
                        item.as_object_mut()
                            .expect("queue item object")
                            .remove("provider_choice_pending");
                    }
                    let _ = crate::utils::queue::save_items(&persisted);
                }
                Err(error.to_string())
            }
        };
    }

    crate::control::deliver_prompt_to_agent(Some(app), state, target, prompt, input_mode)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ActiveAgent, AgentWatchState, AppState};
    use std::sync::{Arc, Mutex};
    use wardian_core::control::WatchTranscriptMessage;
    use wardian_core::models::AgentConfig;

    fn test_agent(
        session_id: &str,
        session_name: &str,
        agent_class: &str,
        status: &str,
    ) -> ActiveAgent {
        ActiveAgent {
            config: Arc::new(Mutex::new(AgentConfig {
                session_id: session_id.to_string(),
                session_name: session_name.to_string(),
                agent_class: agent_class.to_string(),
                provider: "mock".to_string(),
                folder: "<absolute-workspace-path>".to_string(),
                ..Default::default()
            })),
            child_process: None,
            background_processes: Vec::new(),
            memory_capability: None,
            runtime_generation: None,
            process_id: Some(1234),
            query_count: Arc::new(Mutex::new(0)),
            init_timestamp: Arc::new(Mutex::new(None)),
            last_query_timestamp: Arc::new(Mutex::new(None)),
            current_status: Arc::new(Mutex::new(status.to_string())),
            last_status_at: Arc::new(Mutex::new(None)),
            watch_state: Arc::new(Mutex::new(AgentWatchState::new(
                session_id.to_string(),
                16,
                8192,
            ))),
            terminal_title: Arc::new(Mutex::new(String::new())),
            last_output_at: Arc::new(Mutex::new(None)),
            log_path: Arc::new(Mutex::new(None)),
            log_last_modified: Arc::new(Mutex::new(None)),
            #[cfg(windows)]
            job_object: None,
        }
    }

    async fn insert_agent(state: &AppState, agent: ActiveAgent) {
        let session_id = agent.config.lock().expect("config").session_id.clone();
        state.agents.lock().await.insert(session_id, agent);
    }

    #[tokio::test]
    async fn remote_agent_roster_maps_agent_summary_fields() {
        let state = AppState::new();
        let agent = test_agent("agent-1", "CoderOne", "Coder", "Idle");
        {
            let mut watch = agent.watch_state.lock().expect("watch state");
            watch.push_transcript(WatchTranscriptMessage {
                role: "assistant".to_string(),
                text: "ready from transcript".to_string(),
                provider: "mock".to_string(),
                turn_id: Some("turn-1".to_string()),
                source: Some("model".to_string()),
            });
        }
        insert_agent(&state, agent).await;

        let roster = remote_agent_roster(&state).await;

        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].session_id, "agent-1");
        assert_eq!(roster[0].session_name, "CoderOne");
        assert_eq!(roster[0].agent_class, "Coder");
        assert_eq!(roster[0].provider, "mock");
        assert_eq!(roster[0].workspace, "<absolute-workspace-path>");
        assert_eq!(roster[0].status, "Idle");
        assert_eq!(roster[0].latest_text, None);
    }

    #[tokio::test]
    async fn remote_agent_roster_preserves_desktop_agent_order() {
        let state = AppState::new();
        insert_agent(&state, test_agent("agent-1", "Alpha", "Coder", "Idle")).await;
        insert_agent(
            &state,
            test_agent("agent-2", "Beta", "Reviewer", "Processing"),
        )
        .await;
        state
            .agent_order
            .lock()
            .await
            .extend(["agent-2".to_string(), "agent-1".to_string()]);

        let roster = remote_agent_roster(&state).await;

        assert_eq!(
            roster
                .iter()
                .map(|agent| agent.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["agent-2", "agent-1"]
        );
    }

    #[tokio::test]
    async fn remote_agent_roster_omits_latest_text_by_default() {
        let state = AppState::new();
        let agent = test_agent("agent-1", "CoderOne", "Coder", "Processing");
        {
            let mut watch = agent.watch_state.lock().expect("watch state");
            watch.push_output(format!("\u{1b}[31m{}\u{1b}[0m", "x".repeat(5000)).as_bytes());
        }
        insert_agent(&state, agent).await;

        let roster = remote_agent_roster(&state).await;

        assert_eq!(roster[0].latest_text, None);
    }

    #[tokio::test]
    async fn remote_watchlist_state_reads_persisted_state_and_prefs() {
        let _guard = crate::utils::wardian_test_env_lock();
        let temp = tempfile::tempdir().expect("temp home");
        let watchlists_dir = temp.path().join("watchlists");
        std::fs::create_dir_all(&watchlists_dir).expect("watchlists dir");
        std::fs::write(
            watchlists_dir.join("index.json"),
            serde_json::json!({
                "version": 2,
                "teams": [{ "id": "team-1", "name": "Core", "agentIds": ["agent-2", "agent-1"] }],
                "watchlists": [{ "id": "main", "name": "Main", "entries": [{ "type": "team", "teamId": "team-1" }] }]
            })
            .to_string(),
        )
        .expect("watchlist json");
        std::fs::write(
            watchlists_dir.join("prefs.json"),
            serde_json::json!({
                "columns": [],
                "sort": null,
                "preserve_team_grouping_when_sorted": false,
                "collapsed_team_ids": ["team-1"]
            })
            .to_string(),
        )
        .expect("prefs json");

        unsafe { std::env::set_var("WARDIAN_HOME", temp.path()) };
        let response = remote_watchlist_state().expect("watchlist response");
        unsafe { std::env::remove_var("WARDIAN_HOME") };

        assert_eq!(response.watchlists[0]["id"], "main");
        assert_eq!(response.teams[0]["agentIds"][0], "agent-2");
        assert_eq!(
            response.prefs.as_ref().expect("prefs")["collapsed_team_ids"][0],
            "team-1"
        );
    }

    #[tokio::test]
    async fn remote_watchlist_state_uses_empty_defaults_for_missing_or_bad_files() {
        let _guard = crate::utils::wardian_test_env_lock();
        let temp = tempfile::tempdir().expect("temp home");
        std::fs::create_dir_all(temp.path().join("watchlists")).expect("watchlists dir");
        std::fs::write(temp.path().join("watchlists/index.json"), "{").expect("bad index");
        std::fs::write(temp.path().join("watchlists/prefs.json"), "{").expect("bad prefs");

        unsafe { std::env::set_var("WARDIAN_HOME", temp.path()) };
        let response = remote_watchlist_state().expect("watchlist response");
        unsafe { std::env::remove_var("WARDIAN_HOME") };

        assert_eq!(response.watchlists, serde_json::json!([]));
        assert_eq!(response.teams, serde_json::json!([]));
        assert!(response.prefs.is_none());
    }

    #[tokio::test]
    async fn remote_queue_items_reads_the_desktop_inbox_file() {
        let _guard = crate::utils::wardian_test_env_lock();
        let temp = tempfile::tempdir().expect("temp home");
        let queue_dir = temp.path().join("queue");
        std::fs::create_dir_all(&queue_dir).expect("queue dir");
        std::fs::write(
            queue_dir.join("items.json"),
            serde_json::json!([{ "id": "desktop-inbox-1", "type": "approval_request" }])
                .to_string(),
        )
        .expect("queue json");

        unsafe { std::env::set_var("WARDIAN_HOME", temp.path()) };
        let items = remote_queue_items(&AppState::new()).await;
        unsafe { std::env::remove_var("WARDIAN_HOME") };

        assert_eq!(items[0]["id"], "desktop-inbox-1");
    }

    #[tokio::test]
    async fn pending_provider_choice_survives_reload_as_uncertain() {
        let _guard = crate::utils::wardian_test_env_lock();
        let temp = tempfile::tempdir().expect("temp home");
        unsafe { std::env::set_var("WARDIAN_HOME", temp.path()) };
        crate::utils::queue::save_items(&[serde_json::json!({
            "id": "action-1",
            "type": "action_needed",
            "read": false,
            "agent_session_id": "agent-1",
            "summary": "Proceed?\n1. Yes",
            "provider_choice_pending": "1",
        })])
        .expect("pending queue");

        let items = remote_queue_items(&AppState::new()).await;
        unsafe { std::env::remove_var("WARDIAN_HOME") };

        assert_eq!(items[0]["provider_choice_pending"], "1");
        assert!(provider_choice_acknowledgement_unresolved(&items[0]));
    }

    #[tokio::test]
    async fn concurrent_queue_mutations_preserve_both_updates_and_valid_json() {
        let _guard = crate::utils::wardian_test_env_lock();
        let temp = tempfile::tempdir().expect("temp home");
        unsafe { std::env::set_var("WARDIAN_HOME", temp.path()) };
        crate::utils::queue::save_items(&[
            serde_json::json!({ "id": "first", "read": false }),
            serde_json::json!({ "id": "second", "read": false }),
        ])
        .expect("initial queue");

        let state = Arc::new(AppState::new());
        let start = Arc::new(tokio::sync::Barrier::new(2));
        let first_state = Arc::clone(&state);
        let first_start = Arc::clone(&start);
        let first = tokio::spawn(async move {
            first_start.wait().await;
            let _queue_guard = first_state.queue_io_lock.lock().await;
            let mut items = persisted_queue_items();
            tokio::task::yield_now().await;
            items[0]["read"] = serde_json::Value::Bool(true);
            save_persisted_queue_items(&items).expect("first queue update");
        });
        let second_state = Arc::clone(&state);
        let second_start = Arc::clone(&start);
        let second = tokio::spawn(async move {
            second_start.wait().await;
            let _queue_guard = second_state.queue_io_lock.lock().await;
            let mut items = persisted_queue_items();
            tokio::task::yield_now().await;
            items[1]["read"] = serde_json::Value::Bool(true);
            save_persisted_queue_items(&items).expect("second queue update");
        });
        first.await.expect("first task");
        second.await.expect("second task");

        let items = persisted_queue_items();
        unsafe { std::env::remove_var("WARDIAN_HOME") };
        assert_eq!(items[0]["read"], true);
        assert_eq!(items[1]["read"], true);
    }

    #[tokio::test]
    async fn desktop_snapshot_merge_preserves_remote_triage() {
        let _guard = crate::utils::wardian_test_env_lock();
        let temp = tempfile::tempdir().expect("temp home");
        unsafe { std::env::set_var("WARDIAN_HOME", temp.path()) };
        let initial = vec![
            serde_json::json!({ "id": "first", "read": false, "provider_choice_sent": "1" }),
            serde_json::json!({ "id": "second", "read": false }),
        ];
        crate::utils::queue::save_items(&initial).expect("initial queue");
        let state = AppState::new();
        *state.queue_loaded_snapshot.lock().await = Some(initial.clone());

        {
            let _queue_guard = state.queue_io_lock.lock().await;
            let mut remote_items = crate::utils::queue::load_items();
            remote_items[0]["read"] = serde_json::Value::Bool(true);
            crate::utils::queue::save_items(&remote_items).expect("remote queue update");
        }

        {
            let _queue_guard = state.queue_io_lock.lock().await;
            let latest = crate::utils::queue::load_items();
            let base = state.queue_loaded_snapshot.lock().await.clone();
            let desktop_snapshot = vec![
                serde_json::json!({ "id": "first", "read": false }),
                serde_json::json!({ "id": "second", "read": true }),
            ];
            let merged = crate::utils::queue::merge_desktop_snapshot(
                base.as_deref(),
                &desktop_snapshot,
                &latest,
            );
            crate::utils::queue::save_items(&merged).expect("merged queue update");
        }

        let items = crate::utils::queue::load_items();
        unsafe { std::env::remove_var("WARDIAN_HOME") };
        assert_eq!(items[0]["read"], true);
        assert_eq!(items[1]["read"], true);
        assert_eq!(items[0]["provider_choice_sent"], "1");
    }

    #[tokio::test]
    async fn desktop_save_without_load_baseline_preserves_remote_triage() {
        let _guard = crate::utils::wardian_test_env_lock();
        let temp = tempfile::tempdir().expect("temp home");
        unsafe { std::env::set_var("WARDIAN_HOME", temp.path()) };
        let initial = vec![serde_json::json!({ "id": "first", "read": false })];
        crate::utils::queue::save_items(&initial).expect("initial queue");
        let state = AppState::new();

        {
            let _queue_guard = state.queue_io_lock.lock().await;
            let mut remote_items = crate::utils::queue::load_items();
            remote_items[0]["read"] = serde_json::Value::Bool(true);
            crate::utils::queue::save_items(&remote_items).expect("remote queue update");
        }

        let latest = crate::utils::queue::load_items();
        let desktop_snapshot = vec![serde_json::json!({ "id": "first", "read": false })];
        let merged = crate::utils::queue::merge_desktop_snapshot(None, &desktop_snapshot, &latest);
        crate::utils::queue::save_items(&merged).expect("desktop queue update");

        let items = crate::utils::queue::load_items();
        unsafe { std::env::remove_var("WARDIAN_HOME") };
        assert_eq!(items[0]["read"], true);
    }

    #[tokio::test]
    async fn baseline_less_desktop_save_does_not_resurrect_remote_dismissal() {
        let _guard = crate::utils::wardian_test_env_lock();
        let temp = tempfile::tempdir().expect("temp home");
        unsafe { std::env::set_var("WARDIAN_HOME", temp.path()) };
        let initial = vec![serde_json::json!({ "id": "dismissed", "read": false })];
        crate::utils::queue::save_items(&initial).expect("initial queue");
        let state = AppState::new();

        {
            let _queue_guard = state.queue_io_lock.lock().await;
            crate::utils::queue::save_items(&[]).expect("remote dismissal");
        }

        let latest = crate::utils::queue::load_items();
        let desktop_snapshot = initial;
        let merged = crate::utils::queue::merge_desktop_snapshot(None, &desktop_snapshot, &latest);
        crate::utils::queue::save_items(&merged).expect("desktop queue update");

        assert!(crate::utils::queue::load_items().is_empty());
        unsafe { std::env::remove_var("WARDIAN_HOME") };
    }

    #[test]
    fn pending_approval_guard_covers_automation_and_manual_approvals() {
        assert!(is_pending_approval(&serde_json::json!({
            "automation_approval": { "run_id": "run-1" }
        })));
        assert!(is_pending_approval(&serde_json::json!({
            "type": "approval_request",
            "notification_status": "awaiting_reply"
        })));
        assert!(!is_pending_approval(&serde_json::json!({
            "type": "approval_request",
            "notification_status": "completed"
        })));
    }

    #[test]
    fn remote_mark_read_guard_rejects_pending_provider_choice() {
        let error = validate_remote_mark_read(&serde_json::json!({
            "type": "action_needed",
            "read": false,
            "provider_choice_pending": "1"
        }))
        .expect_err("pending provider choice must remain unread");
        assert_eq!(
            error,
            "provider_choice_delivery_uncertain_cannot_be_marked_read"
        );

        assert!(validate_remote_mark_read(&serde_json::json!({
            "type": "action_needed",
            "read": false,
            "provider_choice_sent": "1"
        }))
        .is_ok());
    }

    #[test]
    fn clear_read_only_targets_legacy_completion_items() {
        assert!(is_clearable_legacy_completion(&serde_json::json!({
            "type": "agent_completed"
        })));
        assert!(is_clearable_legacy_completion(&serde_json::json!({
            "type": "automation_completed"
        })));
        assert!(!is_clearable_legacy_completion(&serde_json::json!({
            "type": "action_needed"
        })));
        assert!(!is_clearable_legacy_completion(&serde_json::json!({
            "type": "agent_update"
        })));
        assert!(!is_clearable_legacy_completion(&serde_json::json!({
            "type": "agent_completed",
            "inbox_notification_id": "notice-1"
        })));
        assert!(!is_clearable_legacy_completion(&serde_json::json!({
            "type": "automation_completed",
            "read": true,
            "provider_choice_pending": "1"
        })));
        assert!(!is_clearable_legacy_completion(&serde_json::json!({
            "type": "automation_completed",
            "read": true,
            "dismissed": true
        })));
        assert!(!is_clearable_legacy_completion(&serde_json::json!({
            "type": "agent_completed",
            "read": false,
            "provider_choice_sent": "1"
        })));
    }

    #[tokio::test]
    async fn remote_queue_items_projects_live_inbox_notifications() {
        let _guard = crate::utils::wardian_test_env_lock();
        let temp = tempfile::tempdir().expect("temp home");
        unsafe { std::env::set_var("WARDIAN_HOME", temp.path()) };
        wardian_core::db::init_db_at_path(&temp.path().join("state.db"))
            .expect("initialize state db");
        let state = AppState::new();
        let notification = state
            .interactions
            .create_notification_durable(
                "agent-1".to_string(),
                wardian_core::control::InboxNotificationPayload {
                    kind: InboxNotificationKind::Approval,
                    title: "Approve deployment".to_string(),
                    body: "Deploy the reviewed change.".to_string(),
                    proposed_action: Some("Deploy".to_string()),
                    risk: Some("Changes production".to_string()),
                    choices: vec!["Approve".to_string(), "Reject".to_string()],
                    expires_at: None,
                },
            )
            .await
            .expect("create notification");

        let items = remote_queue_items(&state).await;
        unsafe { std::env::remove_var("WARDIAN_HOME") };

        let item = items
            .iter()
            .find(|item| item["inbox_notification_id"] == notification.id)
            .expect("live inbox notification");
        assert_eq!(item["type"], "approval_request");
        assert_eq!(item["notification_title"], "Approve deployment");
        assert_eq!(
            item["approval_choices"],
            serde_json::json!(["Approve", "Reject"])
        );
    }

    #[tokio::test]
    async fn remote_agent_chat_transcript_returns_normalized_messages() {
        let state = AppState::new();
        let agent = test_agent("agent-1", "CoderOne", "Coder", "Idle");
        {
            let mut watch = agent.watch_state.lock().expect("watch state");
            watch.push_transcript(WatchTranscriptMessage {
                role: "assistant".to_string(),
                text: "Use the shared chat transcript model.".to_string(),
                provider: "mock".to_string(),
                turn_id: Some("turn-1".to_string()),
                source: Some("model".to_string()),
            });
        }
        insert_agent(&state, agent).await;

        let transcript = remote_agent_chat_transcript(&state, "agent-1")
            .await
            .expect("remote chat transcript");

        assert!(transcript.iter().any(|event| {
            event.kind == wardian_core::models::chat::AgentChatEventKind::Message
                && event.role == Some(wardian_core::models::chat::AgentChatRole::Assistant)
                && event.text.as_deref() == Some("Use the shared chat transcript model.")
        }));
    }

    #[test]
    fn unresolved_provider_choice_cannot_be_dismissed() {
        assert!(provider_choice_acknowledgement_unresolved(
            &serde_json::json!({
                "provider_choice_pending": "1",
                "read": false
            })
        ));
        assert!(provider_choice_acknowledgement_unresolved(
            &serde_json::json!({
                "provider_choice_sent": "1",
                "read": false
            })
        ));
        assert!(!provider_choice_acknowledgement_unresolved(
            &serde_json::json!({
                "provider_choice_sent": "1",
                "read": true
            })
        ));
    }

    #[test]
    fn remote_agent_chat_pages_keep_the_newest_events_and_a_stable_older_cursor() {
        let events: Vec<AgentChatEvent> = (1..=85)
            .map(|sequence| AgentChatEvent {
                id: format!("event-{sequence}"),
                session_id: "agent-1".to_string(),
                provider: "mock".to_string(),
                kind: wardian_core::models::chat::AgentChatEventKind::Message,
                role: Some(wardian_core::models::chat::AgentChatRole::Assistant),
                text: Some(format!("Message {sequence}")),
                title: None,
                status: None,
                turn_id: None,
                source: None,
                command: None,
                exit_code: None,
                path: None,
                language: None,
                created_at: None,
                sequence: Some(sequence),
                metadata: serde_json::json!({}),
            })
            .collect();

        let latest = page_remote_agent_chat_events(events.clone(), None, 40);

        assert_eq!(latest.events.len(), 40);
        assert_eq!(
            latest.events.first().map(|event| event.id.as_str()),
            Some("event-46")
        );
        assert_eq!(
            latest.events.last().map(|event| event.id.as_str()),
            Some("event-85")
        );
        assert!(latest.has_older);
        assert_eq!(latest.next_before, Some(45));

        let older = page_remote_agent_chat_events(events.clone(), latest.next_before, 40);
        assert_eq!(older.events.len(), 40);
        assert_eq!(
            older.events.first().map(|event| event.id.as_str()),
            Some("event-6")
        );
        assert_eq!(
            older.events.last().map(|event| event.id.as_str()),
            Some("event-45")
        );
        assert!(older.has_older);
        assert_eq!(older.next_before, Some(5));

        let first_page = page_remote_agent_chat_events(events, older.next_before, 40);
        assert_eq!(first_page.events.len(), 5);
        assert_eq!(
            first_page.events.first().map(|event| event.id.as_str()),
            Some("event-1")
        );
        assert!(!first_page.has_older);
        assert_eq!(first_page.next_before, None);
    }

    #[tokio::test]
    async fn remote_agent_terminal_snapshot_returns_sanitized_output_without_draining() {
        let state = AppState::new();
        let agent = test_agent("agent-1", "CoderOne", "Coder", "Processing");
        {
            let mut watch = agent.watch_state.lock().expect("watch state");
            watch.push_output(b"\x1b[31mred terminal\x1b[0m\nsecond line");
        }
        insert_agent(&state, agent).await;

        let first = remote_agent_terminal_snapshot(&state, "agent-1", None, Some(4096))
            .await
            .expect("first terminal snapshot");
        let second = remote_agent_terminal_snapshot(&state, "agent-1", None, Some(4096))
            .await
            .expect("second terminal snapshot");

        assert_eq!(first.text, "red terminal\nsecond line");
        assert_eq!(second.text, first.text);
        assert_eq!(second.cursor, first.cursor);
        assert!(!first.truncated);
        assert_eq!(first.omitted_bytes, 0);
    }

    #[tokio::test]
    async fn remote_agent_terminal_snapshot_respects_tail_bytes() {
        let state = AppState::new();
        let agent = test_agent("agent-1", "CoderOne", "Coder", "Processing");
        {
            let mut watch = agent.watch_state.lock().expect("watch state");
            watch.push_output(b"alpha beta gamma");
        }
        insert_agent(&state, agent).await;

        let snapshot = remote_agent_terminal_snapshot(&state, "agent-1", None, Some(5))
            .await
            .expect("bounded terminal snapshot");

        assert_eq!(snapshot.text, "gamma");
        assert!(snapshot.truncated);
        assert!(snapshot.omitted_bytes > 0);
    }

    #[tokio::test]
    async fn remote_agent_terminal_raw_output_preserves_escape_sequences_without_draining() {
        let state = AppState::new();
        let agent = test_agent("agent-1", "CoderOne", "Coder", "Processing");
        {
            let mut watch = agent.watch_state.lock().expect("watch state");
            watch.push_output(b"\x1b[31mred terminal\x1b[0m\nsecond line");
        }
        insert_agent(&state, agent).await;

        let first = remote_agent_terminal_raw_output(&state, "agent-1", Some(4096))
            .await
            .expect("first raw terminal output");
        let second = remote_agent_terminal_raw_output(&state, "agent-1", Some(4096))
            .await
            .expect("second raw terminal output");

        assert_eq!(first, "\x1b[31mred terminal\x1b[0m\nsecond line");
        assert_eq!(second, first);
    }

    #[tokio::test]
    async fn remote_agent_terminal_snapshot_rejects_unknown_agent() {
        let state = AppState::new();

        assert_eq!(
            remote_agent_terminal_snapshot(&state, "missing-agent", None, Some(4096))
                .await
                .unwrap_err(),
            "agent_not_found"
        );
    }

    #[test]
    fn run_remote_agent_action_rejects_unknown_actions_before_dispatch() {
        let request = crate::remote::models::RemoteAgentActionRequest {
            action: "open_shell".to_string(),
            target: "agent-1".to_string(),
            prompt: None,
            input_mode: None,
            inbox_item_id: None,
        };

        assert_eq!(
            validate_remote_agent_action(&request).unwrap_err(),
            "unsupported_remote_agent_action"
        );
    }

    #[test]
    fn remote_send_prompt_accepts_command_mode() {
        let request = crate::remote::models::RemoteAgentActionRequest {
            action: "send_prompt".to_string(),
            target: "agent-1".to_string(),
            prompt: Some("/status".to_string()),
            input_mode: Some(wardian_core::control::MessageInputMode::Command),
            inbox_item_id: None,
        };

        validate_remote_agent_action(&request).expect("command mode should be accepted");
    }

    #[test]
    fn remote_send_prompt_rejects_approval_action_mode() {
        let request = crate::remote::models::RemoteAgentActionRequest {
            action: "send_prompt".to_string(),
            target: "agent-1".to_string(),
            prompt: Some("1".to_string()),
            input_mode: Some(wardian_core::control::MessageInputMode::ApprovalAction),
            inbox_item_id: None,
        };

        assert_eq!(
            validate_remote_agent_action(&request).unwrap_err(),
            "unsupported_remote_input_mode"
        );
    }

    #[test]
    fn remote_agent_action_rejects_clone() {
        let request = crate::remote::models::RemoteAgentActionRequest {
            action: "clone".to_string(),
            target: "agent-1".to_string(),
            prompt: None,
            input_mode: None,
            inbox_item_id: None,
        };

        assert_eq!(
            validate_remote_agent_action(&request).unwrap_err(),
            "unsupported_remote_agent_action"
        );
    }
}

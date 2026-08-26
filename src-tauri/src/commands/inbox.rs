use crate::{state::AppState, workflow::runs};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use wardian_core::control::{
    InboxNotificationDecision, InboxNotificationKind, InboxNotificationPayload, InteractionBodyRef,
    InteractionStatus,
};

#[derive(Debug, Clone, Serialize)]
pub struct InboxNotificationDto {
    pub id: String,
    pub kind: InboxNotificationKind,
    pub sender_session_id: String,
    pub status: InteractionStatus,
    pub title: String,
    pub body: String,
    pub proposed_action: Option<String>,
    pub risk: Option<String>,
    pub choices: Vec<String>,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub decision: Option<InboxNotificationDecision>,
}

pub const MAX_INBOX_NOTIFICATIONS: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub struct InboxNotificationListResult {
    pub notifications: Vec<InboxNotificationDto>,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowInboxApprovalDto {
    pub blueprint_id: String,
    pub blueprint_path: String,
    pub run_id: String,
    pub node: String,
    pub title: String,
    pub prompt: String,
    pub created_at: Option<String>,
}

/// Terminal workflow runs are durable Inbox evidence. The frontend uses this
/// reconciliation query at startup, while `workflow-inbox-updated` remains the
/// low-latency path for a currently open window.
#[tauri::command]
pub async fn list_workflow_inbox_terminal_runs() -> Result<Vec<runs::WorkflowInboxUpdate>, String> {
    tokio::task::spawn_blocking(list_workflow_inbox_terminal_runs_blocking)
        .await
        .map_err(|error| format!("workflow terminal inbox task failed: {error}"))?
}

fn list_workflow_inbox_terminal_runs_blocking() -> Result<Vec<runs::WorkflowInboxUpdate>, String> {
    let mut updates = Vec::new();
    for run in crate::commands::workflow::workflow_list_runs_blocking(None)?.runs {
        let Some(run_root) = run.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(workflow_id) = run.get("blueprint_id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let workflow_name = run
            .get("blueprint_path")
            .and_then(serde_json::Value::as_str)
            .and_then(|path| wardian_core::workflow::parse_file(std::path::Path::new(path)).ok())
            .map(|blueprint| blueprint.name)
            .unwrap_or_else(|| workflow_id.to_string());
        let Some(update) =
            runs::workflow_inbox_update_with_name(&workflow_name, std::path::Path::new(run_root))
        else {
            continue;
        };
        if matches!(update.status.as_str(), "completed" | "failed") {
            updates.push(update);
        }
    }
    Ok(updates)
}

#[tauri::command]
pub async fn list_inbox_notifications(
    state: State<'_, AppState>,
    offset: Option<usize>,
) -> Result<InboxNotificationListResult, String> {
    list_inbox_notifications_for_state_with_offset(&state, offset.unwrap_or(0)).await
}

pub async fn list_inbox_notifications_for_state(
    state: &AppState,
) -> Result<InboxNotificationListResult, String> {
    list_inbox_notifications_for_state_with_offset(state, 0).await
}

pub async fn list_inbox_notifications_for_state_with_offset(
    state: &AppState,
    offset: usize,
) -> Result<InboxNotificationListResult, String> {
    let (records, truncated) = state
        .interactions
        .inbox_notifications_page(offset, MAX_INBOX_NOTIFICATIONS)
        .await;
    let mut notifications = Vec::new();
    for record in records {
        let record = state
            .interactions
            .expire_notification_if_needed(&record.id)
            .await
            .unwrap_or(record);
        let Some(payload) = notification_payload(&record) else {
            continue;
        };
        let Some(sender_session_id) = record.sender_session_id.clone() else {
            continue;
        };
        notifications.push(InboxNotificationDto {
            id: record.id.clone(),
            kind: payload.kind,
            sender_session_id,
            status: record.status,
            title: payload.title,
            body: payload.body,
            proposed_action: payload.proposed_action,
            risk: payload.risk,
            choices: payload.choices,
            expires_at: payload.expires_at,
            created_at: record.created_at,
            decision: state.interactions.notification_decision(&record.id).await,
        });
    }
    notifications.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(InboxNotificationListResult {
        notifications,
        truncated,
        next_offset: truncated.then_some(offset + MAX_INBOX_NOTIFICATIONS),
    })
}

#[tauri::command]
pub async fn resolve_inbox_notification(
    state: State<'_, AppState>,
    app: AppHandle,
    notification_id: String,
    choice: String,
) -> Result<InboxNotificationDecision, String> {
    let decision = state
        .interactions
        .resolve_notification(&notification_id, &choice)
        .await
        .map_err(notification_error)?;
    let _ = app.emit("inbox-updated", ());
    Ok(decision)
}

#[tauri::command]
pub async fn list_workflow_inbox_approvals() -> Result<Vec<WorkflowInboxApprovalDto>, String> {
    tokio::task::spawn_blocking(list_workflow_inbox_approvals_blocking)
        .await
        .map_err(|error| format!("workflow approval inbox task failed: {error}"))?
}

fn list_workflow_inbox_approvals_blocking() -> Result<Vec<WorkflowInboxApprovalDto>, String> {
    let runs = crate::commands::workflow::workflow_list_runs_blocking(None)?.runs;
    let mut approvals = Vec::new();
    for run in runs {
        if run.get("status").and_then(serde_json::Value::as_str) != Some("awaiting_approval") {
            continue;
        }
        let Some(blueprint_id) = run.get("blueprint_id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(run_id) = run.get("run_id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(blueprint_path) = run
            .get("blueprint_path")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let detail = crate::commands::workflow::workflow_read_run(
            blueprint_id.to_string(),
            run_id.to_string(),
        )?;
        let Some(node) = detail
            .get("events")
            .and_then(serde_json::Value::as_array)
            .and_then(|events| {
                events.iter().rev().find_map(|event| {
                    (event.get("kind").and_then(serde_json::Value::as_str)
                        == Some("awaiting_approval"))
                    .then(|| event.get("node").and_then(serde_json::Value::as_str))
                    .flatten()
                })
            })
        else {
            continue;
        };
        let blueprint: wardian_core::workflow::Blueprint = serde_json::from_value(
            detail
                .get("blueprint")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )
        .map_err(|_| "could not read workflow approval blueprint".to_string())?;
        let Some(approval_node) = blueprint
            .nodes
            .iter()
            .find(|candidate| candidate.id == node)
        else {
            continue;
        };
        let prompt = approval_node
            .fields
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Approve this workflow step?")
            .to_string();
        approvals.push(WorkflowInboxApprovalDto {
            blueprint_id: blueprint_id.to_string(),
            blueprint_path: blueprint_path.to_string(),
            run_id: run_id.to_string(),
            node: node.to_string(),
            title: approval_node
                .name
                .clone()
                .unwrap_or_else(|| format!("{} approval", blueprint.name)),
            prompt,
            created_at: run
                .get("updated_at")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        });
    }
    Ok(approvals)
}

fn notification_payload(
    record: &wardian_core::control::InteractionRecord,
) -> Option<InboxNotificationPayload> {
    let InteractionBodyRef::Inline { body } = &record.body_ref else {
        return None;
    };
    serde_json::from_str(body).ok()
}

fn notification_error(error: &'static str) -> String {
    match error {
        "not_found" => "Inbox notification was not found".to_string(),
        "not_notification" | "not_approval" => "Inbox item is not an approval request".to_string(),
        "already_resolved" => "Approval was already resolved".to_string(),
        "expired" => "Approval expired without a decision".to_string(),
        "invalid_choice" => "That approval choice is not available".to_string(),
        "persistence_failed" => "Could not persist approval decision".to_string(),
        _ => "Could not resolve Inbox approval".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardian_core::engine::{
        store::{append_event, write_checkpoint},
        Event, EventKind, RunState, RunStatus,
    };

    struct EnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous_home: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(home: &std::path::Path) -> Self {
            let guard = Self {
                _lock: crate::utils::wardian_test_env_lock(),
                previous_home: std::env::var_os("WARDIAN_HOME"),
            };
            std::env::set_var("WARDIAN_HOME", home);
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.previous_home.take() {
                Some(value) => std::env::set_var("WARDIAN_HOME", value),
                None => std::env::remove_var("WARDIAN_HOME"),
            }
        }
    }

    #[tokio::test]
    async fn terminal_run_query_includes_a_missing_scheduled_blueprint_with_id_fallback() {
        let home = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(home.path());
        let run_root = home
            .path()
            .join("logs")
            .join("workflows")
            .join("missing-scheduled-workflow")
            .join("run-1");
        let mut state = RunState::new("run-1", "missing-scheduled-workflow");
        state.status = RunStatus::Failed;
        state.failure = Some("workflow blueprint was removed".to_string());
        write_checkpoint(&run_root, &state).unwrap();
        append_event(
            &run_root,
            &Event::new(
                0,
                EventKind::RunFailed {
                    error: "workflow blueprint was removed".to_string(),
                },
            ),
        )
        .unwrap();

        let updates = list_workflow_inbox_terminal_runs().await.unwrap();

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].workflow_id, "missing-scheduled-workflow");
        assert_eq!(updates[0].workflow_name, "missing-scheduled-workflow");
        assert_eq!(updates[0].status, "failed");
        assert_eq!(
            updates[0].error.as_deref(),
            Some("workflow blueprint was removed")
        );
    }
}

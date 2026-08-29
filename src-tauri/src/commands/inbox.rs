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
    Ok(list_workflow_inbox_terminal_runs_page(0).await?.0)
}

/// Lists one bounded page of terminal workflow Inbox evidence and preserves
/// the workflow-run continuation state for other projections.
pub async fn list_workflow_inbox_terminal_runs_page(
    offset: usize,
) -> Result<(Vec<runs::WorkflowInboxUpdate>, bool), String> {
    tokio::task::spawn_blocking(move || list_workflow_inbox_terminal_runs_blocking(offset))
        .await
        .map_err(|error| format!("workflow terminal inbox task failed: {error}"))?
}

fn list_workflow_inbox_terminal_runs_blocking(
    offset: usize,
) -> Result<(Vec<runs::WorkflowInboxUpdate>, bool), String> {
    let mut updates = Vec::new();
    let (runs, truncated) = workflow_inbox_run_page(offset, |run| {
        matches!(
            run.get("status").and_then(serde_json::Value::as_str),
            Some("completed" | "failed")
        )
    })?;
    for run in runs {
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
    Ok((updates, truncated))
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
    list_inbox_notifications_for_state_with_offset_internal(state, offset, true).await
}

/// Reads notification projections without expiring or otherwise mutating
/// durable records. This is used by read-only agent and remote Inbox paths.
pub async fn list_inbox_notifications_for_state_with_offset_read_only(
    state: &AppState,
    offset: usize,
) -> Result<InboxNotificationListResult, String> {
    list_inbox_notifications_for_state_with_offset_internal(state, offset, false).await
}

async fn list_inbox_notifications_for_state_with_offset_internal(
    state: &AppState,
    offset: usize,
    expire_records: bool,
) -> Result<InboxNotificationListResult, String> {
    let (records, truncated) = state
        .interactions
        .inbox_notifications_page(offset, MAX_INBOX_NOTIFICATIONS)
        .await;
    let mut notifications = Vec::new();
    for record in records {
        let mut record = if expire_records {
            state
                .interactions
                .expire_notification_if_needed(&record.id)
                .await
                .unwrap_or(record)
        } else {
            record
        };
        let Some(payload) = notification_payload(&record) else {
            continue;
        };
        if !expire_records
            && record.status == InteractionStatus::AwaitingReply
            && notification_expired(&payload)
        {
            // Preserve the projection users would see after normal expiry,
            // without persisting a mutation from a read-only command.
            record.status = InteractionStatus::Expired;
        }
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
    Ok(list_workflow_inbox_approvals_page(0).await?.0)
}

/// Lists one bounded page of awaiting workflow approval evidence.
pub async fn list_workflow_inbox_approvals_page(
    offset: usize,
) -> Result<(Vec<WorkflowInboxApprovalDto>, bool), String> {
    tokio::task::spawn_blocking(move || list_workflow_inbox_approvals_blocking(offset))
        .await
        .map_err(|error| format!("workflow approval inbox task failed: {error}"))?
}

fn list_workflow_inbox_approvals_blocking(
    offset: usize,
) -> Result<(Vec<WorkflowInboxApprovalDto>, bool), String> {
    let (runs, truncated) = workflow_inbox_run_page(offset, |run| {
        run.get("status").and_then(serde_json::Value::as_str) == Some("awaiting_approval")
    })?;
    let mut approvals = Vec::new();
    for run in runs {
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
    Ok((approvals, truncated))
}

/// Pages the eligible workflow Inbox projection rather than applying the
/// caller's offset to all workflow runs before filtering. This keeps older
/// approvals and terminal outcomes reachable when other workflow states are
/// interleaved with them.
fn workflow_inbox_run_page<F>(
    offset: usize,
    include: F,
) -> Result<(Vec<serde_json::Value>, bool), String>
where
    F: FnMut(&serde_json::Value) -> bool,
{
    page_workflow_inbox_runs(
        offset,
        |raw_offset| crate::commands::workflow::workflow_list_runs_blocking(Some(raw_offset)),
        include,
    )
}

fn page_workflow_inbox_runs<L, F>(
    offset: usize,
    mut load_page: L,
    mut include: F,
) -> Result<(Vec<serde_json::Value>, bool), String>
where
    L: FnMut(usize) -> Result<crate::commands::workflow::WorkflowRunListResult, String>,
    F: FnMut(&serde_json::Value) -> bool,
{
    let target = offset
        .saturating_add(MAX_INBOX_NOTIFICATIONS)
        .saturating_add(1);
    let mut eligible = Vec::with_capacity(target.min(MAX_INBOX_NOTIFICATIONS + 1));
    let mut raw_offset = 0;

    loop {
        let page = load_page(raw_offset)?;
        for run in page.runs {
            if include(&run) {
                eligible.push(run);
                if eligible.len() >= target {
                    break;
                }
            }
        }
        if eligible.len() >= target || !page.truncated {
            break;
        }
        raw_offset = page
            .next_offset
            .unwrap_or_else(|| raw_offset.saturating_add(MAX_INBOX_NOTIFICATIONS));
    }

    let page_end = offset.saturating_add(MAX_INBOX_NOTIFICATIONS);
    let truncated = eligible.len() > page_end;
    Ok((
        eligible
            .into_iter()
            .skip(offset)
            .take(MAX_INBOX_NOTIFICATIONS)
            .collect(),
        truncated,
    ))
}

fn notification_payload(
    record: &wardian_core::control::InteractionRecord,
) -> Option<InboxNotificationPayload> {
    let InteractionBodyRef::Inline { body } = &record.body_ref else {
        return None;
    };
    serde_json::from_str(body).ok()
}

fn notification_expired(payload: &InboxNotificationPayload) -> bool {
    let Some(expires_at) = payload.expires_at.as_deref() else {
        return false;
    };
    let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(expires_at) else {
        return true;
    };
    expires_at <= chrono::Utc::now()
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

    #[test]
    fn workflow_inbox_paging_filters_before_applying_offset() {
        let (runs, truncated) = page_workflow_inbox_runs(
            0,
            |offset| {
                if offset == 0 {
                    Ok(crate::commands::workflow::WorkflowRunListResult {
                        runs: (0..200)
                            .map(|index| {
                                serde_json::json!({
                                    "run_id": format!("non-inbox-{index}"),
                                    "status": "running",
                                })
                            })
                            .collect(),
                        truncated: true,
                        next_offset: Some(200),
                    })
                } else {
                    Ok(crate::commands::workflow::WorkflowRunListResult {
                        runs: vec![serde_json::json!({
                            "run_id": "inbox-run",
                            "status": "awaiting_approval",
                        })],
                        truncated: false,
                        next_offset: None,
                    })
                }
            },
            |run| {
                run.get("status").and_then(serde_json::Value::as_str) == Some("awaiting_approval")
            },
        )
        .unwrap();

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0]["run_id"], "inbox-run");
        assert!(!truncated);
    }
}

use crate::{
    args::{InboxArgs, InboxCommand},
    errors::{CliError, ExitCode},
    live,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
};
use wardian_core::control::{
    InboxNotificationDecision, InboxNotificationPayload, InteractionBodyRef, InteractionKind,
    InteractionRecord, InteractionStatus,
};
use wardian_core::engine::{EventKind, RunStatus};

/// Read-only Inbox commands. Live reads use the app-owned projection first so
/// the CLI and remote Inbox see the same notification and workflow records.
pub fn handle_inbox(args: InboxArgs) -> Result<String, CliError> {
    match args.command {
        InboxCommand::List {
            types,
            sources,
            unread,
            limit,
            offset,
        } => {
            if limit == 0 {
                return Err(CliError::backend(
                    ExitCode::Generic,
                    "invalid_limit",
                    "--limit must be greater than zero",
                ));
            }
            let types = normalize_filter(types, "--type")?;
            let sources = normalize_filter(sources, "--source")?;
            let items = live::inbox_list().or_else(|_| load_persisted_items())?;
            render_list(&items, &types, &sources, unread, limit, offset)
        }
    }
}

fn normalize_filter(values: Vec<String>, flag: &str) -> Result<HashSet<String>, CliError> {
    let values = values
        .into_iter()
        .flat_map(|value| value.split(',').map(str::to_string).collect::<Vec<_>>())
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if values.iter().any(String::is_empty) {
        return Err(CliError::backend(
            ExitCode::Generic,
            "invalid_filter",
            format!("{flag} values must not be empty"),
        ));
    }
    Ok(values.into_iter().collect())
}

fn render_list(
    items: &[Value],
    types: &HashSet<String>,
    sources: &HashSet<String>,
    unread: bool,
    limit: usize,
    offset: usize,
) -> Result<String, CliError> {
    let mut filtered = items
        .iter()
        .filter(|item| type_matches(item, types))
        .filter(|item| source_matches(item, sources))
        .filter(|item| !unread || item.get("read").and_then(Value::as_bool) != Some(true))
        .cloned()
        .collect::<Vec<_>>();
    filtered.sort_by(|left, right| {
        item_timestamp(right)
            .cmp(&item_timestamp(left))
            .then_with(|| item_id(right).cmp(item_id(left)))
    });

    let end = offset.saturating_add(limit).min(filtered.len());
    let page = if offset < filtered.len() {
        filtered[offset..end].to_vec()
    } else {
        Vec::new()
    };
    let truncated = end < filtered.len();
    let response = json!({
        "schema": 1,
        "items": page,
        "truncated": truncated,
        "next_offset": truncated.then_some(end),
    });
    serde_json::to_string_pretty(&response)
        .map(|json| format!("{json}\n"))
        .map_err(|error| CliError::generic(error.to_string()))
}

fn type_matches(item: &Value, types: &HashSet<String>) -> bool {
    if types.is_empty() {
        return true;
    }
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
    types.contains(item_type)
        || (item_type == "workflow_completed"
            && item.get("status").and_then(Value::as_str) == Some("failed")
            && types.contains("workflow_failed"))
}

fn source_matches(item: &Value, sources: &HashSet<String>) -> bool {
    sources.is_empty()
        || sources.contains(
            item.get("evidence_source")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
}

fn item_timestamp(item: &Value) -> i64 {
    item.get("timestamp")
        .and_then(Value::as_i64)
        .or_else(|| {
            item.get("timestamp")
                .and_then(Value::as_u64)
                .and_then(|value| i64::try_from(value).ok())
        })
        .unwrap_or_default()
}

fn item_id(item: &Value) -> &str {
    item.get("id").and_then(Value::as_str).unwrap_or_default()
}

fn load_persisted_items() -> Result<Vec<Value>, CliError> {
    let Some(home) = wardian_core::paths::wardian_home() else {
        return Ok(Vec::new());
    };
    let queue_path = home.join("queue").join("items.json");
    let persisted = if queue_path.exists() {
        let content = std::fs::read_to_string(&queue_path)
            .map_err(|error| CliError::generic(format!("could not read Inbox items: {error}")))?;
        serde_json::from_str::<Vec<Value>>(&content)
            .map_err(|error| CliError::generic(format!("could not parse Inbox items: {error}")))?
    } else {
        Vec::new()
    };
    let read_notification_ids = persisted
        .iter()
        .filter(|item| {
            item.get("type").and_then(Value::as_str) == Some("agent_update")
                && item.get("read").and_then(Value::as_bool) == Some(true)
        })
        .filter_map(|item| {
            item.get("inbox_notification_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<HashSet<_>>();
    let mut items = persisted
        .into_iter()
        .filter(|item| {
            item.get("inbox_notification_id").is_none()
                && item.get("workflow_approval").is_none()
                && item.get("dismissed").is_none()
        })
        .collect::<Vec<_>>();
    items.extend(workflow_approval_items()?);

    let Some(db_path) = wardian_core::paths::state_db_path() else {
        return Ok(sort_items(items));
    };
    if !db_path.exists() {
        return Ok(sort_items(items));
    }
    let conn =
        Connection::open(&db_path).map_err(|error| CliError::db_unavailable(error.to_string()))?;
    wardian_core::db::run_migrations(&conn)
        .map_err(|error| CliError::db_unavailable(error.to_string()))?;
    let records = wardian_core::db::list_interaction_records_with_conn(&conn)
        .map_err(|error| CliError::db_unavailable(error.to_string()))?;
    let agent_names = wardian_core::db::get_all_agents_with_conn(&conn)
        .map_err(|error| CliError::db_unavailable(error.to_string()))?
        .into_iter()
        .map(|agent| (agent.session_id, agent.session_name))
        .collect::<HashMap<_, _>>();
    items.extend(notification_items(
        &records,
        &read_notification_ids,
        &agent_names,
    ));
    Ok(sort_items(items))
}

fn workflow_approval_items() -> Result<Vec<Value>, CliError> {
    let Some(runs_root) = wardian_core::paths::workflow_runs_dir() else {
        return Ok(Vec::new());
    };
    if !runs_root.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for blueprint_entry in
        fs::read_dir(&runs_root).map_err(|error| CliError::generic(error.to_string()))?
    {
        let blueprint_dir = blueprint_entry
            .map_err(|error| CliError::generic(error.to_string()))?
            .path();
        if !blueprint_dir.is_dir() {
            continue;
        }
        for run_entry in
            fs::read_dir(&blueprint_dir).map_err(|error| CliError::generic(error.to_string()))?
        {
            let run_dir = run_entry
                .map_err(|error| CliError::generic(error.to_string()))?
                .path();
            if !run_dir.is_dir() {
                continue;
            }
            let Ok(Some(state)) = wardian_core::engine::store::read_checkpoint(&run_dir) else {
                continue;
            };
            if state.status != RunStatus::AwaitingApproval {
                continue;
            }
            let Ok(events) = wardian_core::engine::store::read_events(&run_dir) else {
                continue;
            };
            let Some((node, timestamp)) = events.iter().rev().find_map(|event| match &event.kind {
                EventKind::AwaitingApproval { node } => Some((node.clone(), event.ts.clone())),
                _ => None,
            }) else {
                continue;
            };

            let blueprint = wardian_core::workflow::resolve_blueprint_path(&state.blueprint_id)
                .and_then(|path| wardian_core::workflow::parse_file(&path).ok());
            let workflow_name = blueprint
                .as_ref()
                .map(|blueprint| blueprint.name.clone())
                .unwrap_or_else(|| state.blueprint_id.clone());
            let approval_node = blueprint
                .as_ref()
                .and_then(|blueprint| blueprint.find_node(&node));
            let title = approval_node
                .and_then(|node| node.name.clone())
                .unwrap_or_else(|| format!("{workflow_name} approval"));
            let prompt = approval_node
                .and_then(|node| node.fields.get("prompt"))
                .and_then(Value::as_str)
                .unwrap_or("Approve this workflow step?");
            let blueprint_path =
                wardian_core::workflow::resolve_blueprint_path(&state.blueprint_id)
                    .map(|path| path.to_string_lossy().into_owned());

            items.push(json!({
                "id": format!("workflow-approval:{}:{}:{}", state.blueprint_id, state.run_id, node),
                "type": "approval_request",
                "timestamp": timestamp_millis(&timestamp),
                "read": false,
                "evidence_source": "live_runtime",
                "workflow_id": state.blueprint_id,
                "workflow_run_id": state.run_id,
                "workflow_name": title,
                "notification_title": title,
                "summary": prompt,
                "proposed_action": "Continue this workflow beyond its approval gate",
                "risk": "The workflow will execute the next authored steps after approval.",
                "approval_choices": ["Approve", "Reject"],
                "workflow_approval": {
                    "blueprint_id": state.blueprint_id,
                    "blueprint_path": blueprint_path.unwrap_or_default(),
                    "run_id": state.run_id,
                    "node": node,
                },
            }));
        }
    }
    Ok(items)
}

fn notification_items(
    records: &[InteractionRecord],
    read_notification_ids: &HashSet<String>,
    agent_names: &HashMap<String, String>,
) -> Vec<Value> {
    records
        .iter()
        .filter(|record| record.kind == InteractionKind::Notification)
        .filter_map(|record| {
            let payload = notification_payload(record)?;
            let sender_session_id = record.sender_session_id.as_deref()?;
            let status = notification_status(record, &payload);
            let is_approval = matches!(payload.kind, wardian_core::control::InboxNotificationKind::Approval);
            let notification_id = record.id.as_str();
            let mut item = json!({
                "id": format!("notification:{notification_id}"),
                "type": if is_approval { "approval_request" } else { "agent_update" },
                "timestamp": timestamp_millis(&record.created_at),
                "read": if is_approval { status != "awaiting_reply" } else { read_notification_ids.contains(notification_id) },
                "agent_session_id": sender_session_id,
                "evidence_source": "interaction_store",
                "inbox_notification_id": notification_id,
                "notification_status": status,
                "notification_title": payload.title,
                "summary": payload.body,
                "proposed_action": payload.proposed_action,
                "risk": payload.risk,
                "approval_choices": payload.choices,
                "expires_at": payload.expires_at,
            });
            if let Some(agent_name) = agent_names.get(sender_session_id) {
                item["agent_name"] = Value::String(agent_name.clone());
            }
            if let Some(decision) = notification_decision(record, records) {
                item["approval_decision"] = Value::String(decision.choice);
            }
            Some(item)
        })
        .collect()
}

fn notification_payload(record: &InteractionRecord) -> Option<InboxNotificationPayload> {
    let InteractionBodyRef::Inline { body } = &record.body_ref else {
        return None;
    };
    serde_json::from_str(body).ok()
}

fn notification_decision(
    notification: &InteractionRecord,
    records: &[InteractionRecord],
) -> Option<InboxNotificationDecision> {
    records
        .iter()
        .find(|record| {
            record.kind == InteractionKind::Reply
                && record.parent_interaction_id.as_deref() == Some(notification.id.as_str())
        })
        .and_then(|record| match &record.body_ref {
            InteractionBodyRef::Inline { body } => serde_json::from_str(body).ok(),
            InteractionBodyRef::File { .. } => None,
        })
}

fn notification_status(
    record: &InteractionRecord,
    payload: &InboxNotificationPayload,
) -> &'static str {
    if record.status == InteractionStatus::AwaitingReply && notification_expired(payload) {
        "expired"
    } else {
        match record.status {
            InteractionStatus::AwaitingReply => "awaiting_reply",
            InteractionStatus::Expired => "expired",
            _ => "completed",
        }
    }
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

fn timestamp_millis(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.timestamp_millis())
        .unwrap_or_default()
}

fn sort_items(mut items: Vec<Value>) -> Vec<Value> {
    items.sort_by(|left, right| {
        item_timestamp(right)
            .cmp(&item_timestamp(left))
            .then_with(|| item_id(right).cmp(item_id(left)))
    });
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_workflows_match_the_workflow_failed_filter_alias() {
        let item = json!({
            "id": "workflow-completion:wf:run-1",
            "type": "workflow_completed",
            "status": "failed",
        });
        let types = HashSet::from(["workflow_failed".to_string()]);

        assert!(type_matches(&item, &types));
    }

    #[test]
    fn filtering_preserves_newest_first_and_paging_metadata() {
        let items = vec![
            json!({"id": "new", "type": "action_needed", "timestamp": 2, "read": false, "evidence_source": "provider_runtime"}),
            json!({"id": "old", "type": "agent_completed", "timestamp": 1, "read": true, "evidence_source": "provider_runtime"}),
        ];
        let types = HashSet::from(["action_needed".to_string(), "agent_completed".to_string()]);
        let sources = HashSet::from(["provider_runtime".to_string()]);

        let output = render_list(&items, &types, &sources, false, 1, 0).unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["items"][0]["id"], "new");
        assert_eq!(output["truncated"], true);
        assert_eq!(output["next_offset"], 1);
    }
}

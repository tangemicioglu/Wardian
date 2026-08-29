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

const MAX_INBOX_SOURCE_ITEMS: usize = wardian_core::control::MAX_INBOX_PAGE_LIMIT;
const QUEUE_MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

struct InboxProjection {
    items: Vec<Value>,
    truncated: bool,
    next_offset: Option<usize>,
}

#[derive(Clone, Copy)]
enum InboxSource {
    Notifications,
    WorkflowApprovals,
    WorkflowTerminals,
    LegacyQueue,
}

struct InboxSourcePage {
    items: Vec<Value>,
    truncated: bool,
}

struct InboxPageRequest<'a> {
    offset: usize,
    limit: usize,
    types: &'a HashSet<String>,
    sources: &'a HashSet<String>,
    unread: bool,
}

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
            if limit > MAX_INBOX_SOURCE_ITEMS {
                return Err(CliError::backend(
                    ExitCode::Generic,
                    "invalid_limit",
                    format!("--limit must not exceed {MAX_INBOX_SOURCE_ITEMS}"),
                ));
            }
            if offset > wardian_core::control::MAX_INBOX_OFFSET {
                return Err(CliError::backend(
                    ExitCode::Generic,
                    "invalid_offset",
                    format!(
                        "--offset must not exceed {}",
                        wardian_core::control::MAX_INBOX_OFFSET
                    ),
                ));
            }
            let types = normalize_filter(types, "--type")?;
            let sources = normalize_filter(sources, "--source")?;
            let projection = live::inbox_list_page(
                offset,
                types.iter().cloned().collect(),
                sources.iter().cloned().collect(),
                unread,
                limit,
            )
            .map(|page| InboxProjection {
                items: page.items,
                truncated: page.truncated,
                next_offset: page.next_offset,
            })
            .or_else(|_| load_persisted_items(offset, &types, &sources, unread, limit))?;
            render_list(&projection)
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

fn render_list(projection: &InboxProjection) -> Result<String, CliError> {
    let response = json!({
        "schema": 1,
        "items": projection.items,
        "truncated": projection.truncated,
        "next_offset": projection.next_offset,
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

fn workflow_identity(item: &Value) -> Option<(String, String)> {
    if item.get("type").and_then(Value::as_str) != Some("workflow_completed") {
        return None;
    }
    Some((
        item.get("workflow_id").and_then(Value::as_str)?.to_string(),
        item.get("workflow_run_id")
            .and_then(Value::as_str)?
            .to_string(),
    ))
}

fn retain_newest<I>(items: I, limit: usize) -> (Vec<Value>, bool)
where
    I: IntoIterator<Item = Value>,
{
    let mut retained = Vec::with_capacity(limit.saturating_add(1));
    let mut truncated = false;
    for item in items {
        retained.push(item);
        retained.sort_by(|left, right| {
            item_timestamp(right)
                .cmp(&item_timestamp(left))
                .then_with(|| item_id(right).cmp(item_id(left)))
        });
        if retained.len() > limit {
            retained.pop();
            truncated = true;
        }
    }
    (retained, truncated)
}

fn load_persisted_items(
    offset: usize,
    types: &HashSet<String>,
    sources: &HashSet<String>,
    unread: bool,
    limit: usize,
) -> Result<InboxProjection, CliError> {
    // The legacy queue is optional. A damaged queue must not hide the durable
    // SQLite notifications or workflow run projections below it.
    let cutoff = chrono::Utc::now().timestamp_millis() - QUEUE_MAX_AGE_MS;
    let metadata = wardian_core::queue::load_recent_items(MAX_INBOX_SOURCE_ITEMS, 0, cutoff);
    let read_notification_ids = metadata.read_notification_ids;
    let persisted_workflow_runs = metadata.workflow_runs;
    let conn = match wardian_core::paths::state_db_path() {
        Some(path) if path.exists() => {
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
        }
        _ => None,
    };
    let agent_names = conn
        .as_ref()
        .map(wardian_core::db::get_all_agents_with_conn)
        .and_then(Result::ok)
        .unwrap_or_default()
        .into_iter()
        .map(|agent| (agent.session_id, agent.session_name))
        .collect::<HashMap<_, _>>();
    let source_kinds = [
        InboxSource::Notifications,
        InboxSource::WorkflowApprovals,
        InboxSource::WorkflowTerminals,
        InboxSource::LegacyQueue,
    ];
    let mut pages = source_kinds
        .into_iter()
        .map(|source| {
            load_persisted_source_page(
                source,
                0,
                cutoff,
                &read_notification_ids,
                &persisted_workflow_runs,
                conn.as_ref(),
                &agent_names,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut source_offsets = [0usize; 4];
    let request = InboxPageRequest {
        offset,
        limit,
        types,
        sources,
        unread,
    };
    let (items, truncated, next_offset) = merge_persisted_pages(
        &request,
        &mut pages,
        &mut |index, source_offset| {
            load_persisted_source_page(
                source_kinds[index],
                source_offset,
                cutoff,
                &read_notification_ids,
                &persisted_workflow_runs,
                conn.as_ref(),
                &agent_names,
            )
        },
        &mut source_offsets,
    )?;
    Ok(InboxProjection {
        items,
        truncated,
        next_offset,
    })
}

fn load_persisted_source_page(
    source: InboxSource,
    offset: usize,
    cutoff: i64,
    read_notification_ids: &HashSet<String>,
    persisted_workflow_runs: &HashSet<(String, String)>,
    conn: Option<&Connection>,
    agent_names: &HashMap<String, String>,
) -> Result<InboxSourcePage, CliError> {
    match source {
        InboxSource::Notifications => {
            let Some(conn) = conn else {
                return Ok(InboxSourcePage {
                    items: Vec::new(),
                    truncated: false,
                });
            };
            let Ok(mut records) =
                wardian_core::db::list_recent_interaction_records_by_kind_with_conn(
                    conn,
                    "notification",
                    MAX_INBOX_SOURCE_ITEMS + 1,
                    offset,
                )
            else {
                return Ok(InboxSourcePage {
                    items: Vec::new(),
                    truncated: false,
                });
            };
            let truncated = records.len() > MAX_INBOX_SOURCE_ITEMS;
            records.truncate(MAX_INBOX_SOURCE_ITEMS);
            let decisions = notification_decisions(conn, &records);
            Ok(InboxSourcePage {
                items: sort_items(notification_items(
                    &records,
                    read_notification_ids,
                    agent_names,
                    &decisions,
                )),
                truncated,
            })
        }
        InboxSource::WorkflowApprovals => {
            let (items, truncated) = workflow_approval_items(offset)?;
            Ok(InboxSourcePage { items, truncated })
        }
        InboxSource::WorkflowTerminals => {
            let (items, truncated) = workflow_terminal_items(offset)?;
            Ok(InboxSourcePage {
                items: items
                    .into_iter()
                    .filter(|item| {
                        workflow_identity(item)
                            .is_none_or(|key| !persisted_workflow_runs.contains(&key))
                    })
                    .collect(),
                truncated,
            })
        }
        InboxSource::LegacyQueue => {
            let persisted =
                wardian_core::queue::load_recent_items(MAX_INBOX_SOURCE_ITEMS, offset, cutoff);
            Ok(InboxSourcePage {
                items: persisted
                    .items
                    .into_iter()
                    .filter(|item| {
                        item.get("inbox_notification_id").is_none()
                            && item.get("workflow_approval").is_none()
                            && item.get("dismissed").and_then(Value::as_bool) != Some(true)
                    })
                    .collect(),
                truncated: persisted.truncated,
            })
        }
    }
}

fn notification_decisions(
    conn: &Connection,
    records: &[InteractionRecord],
) -> HashMap<String, InboxNotificationDecision> {
    let mut decisions = HashMap::new();
    for record in records {
        let Some(reply) =
            wardian_core::db::list_interaction_replies_for_parent_with_conn(conn, &record.id)
                .ok()
                .and_then(|replies| replies.into_iter().next())
        else {
            continue;
        };
        if let Some(decision) = match &reply.body_ref {
            InteractionBodyRef::Inline { body } => serde_json::from_str(body).ok(),
            InteractionBodyRef::File { .. } => None,
        } {
            decisions.insert(record.id.clone(), decision);
        }
    }
    decisions
}

fn merge_persisted_pages<F>(
    request: &InboxPageRequest<'_>,
    pages: &mut [InboxSourcePage],
    refill: &mut F,
    source_offsets: &mut [usize],
) -> Result<(Vec<Value>, bool, Option<usize>), CliError>
where
    F: FnMut(usize, usize) -> Result<InboxSourcePage, CliError>,
{
    let mut skipped = 0usize;
    let mut items = Vec::with_capacity(request.limit);
    loop {
        for index in 0..pages.len() {
            if pages[index].items.is_empty() && pages[index].truncated {
                source_offsets[index] =
                    source_offsets[index].saturating_add(MAX_INBOX_SOURCE_ITEMS);
                pages[index] = refill(index, source_offsets[index])?;
            }
        }
        let Some(source_index) = pages
            .iter()
            .enumerate()
            .filter_map(|(index, page)| {
                page.items
                    .first()
                    .map(|item| (index, item_timestamp(item), item_id(item)))
            })
            .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.2.cmp(right.2)))
            .map(|candidate| candidate.0)
        else {
            break;
        };

        let item = pages[source_index].items.remove(0);
        if !inbox_item_matches(&item, request.types, request.sources, request.unread) {
            continue;
        }
        if skipped < request.offset {
            skipped += 1;
            continue;
        }
        if items.len() >= request.limit {
            return Ok((
                items,
                true,
                Some(request.offset.saturating_add(request.limit)),
            ));
        }
        items.push(item);
    }
    Ok((items, false, None))
}

fn inbox_item_matches(
    item: &Value,
    types: &HashSet<String>,
    sources: &HashSet<String>,
    unread: bool,
) -> bool {
    type_matches(item, types)
        && source_matches(item, sources)
        && (!unread || item.get("read").and_then(Value::as_bool) != Some(true))
}

fn page_source_items(items: Vec<Value>, truncated: bool, offset: usize) -> (Vec<Value>, bool) {
    let page_end = offset.saturating_add(MAX_INBOX_SOURCE_ITEMS);
    let truncated = truncated || items.len() > page_end;
    (
        items
            .into_iter()
            .skip(offset)
            .take(MAX_INBOX_SOURCE_ITEMS)
            .collect(),
        truncated,
    )
}

fn workflow_approval_items(offset: usize) -> Result<(Vec<Value>, bool), CliError> {
    let Some(runs_root) = wardian_core::paths::workflow_runs_dir() else {
        return Ok((Vec::new(), false));
    };
    if !runs_root.exists() {
        return Ok((Vec::new(), false));
    }

    let mut items = Vec::new();
    let mut truncated = false;
    let capacity = offset
        .saturating_add(MAX_INBOX_SOURCE_ITEMS)
        .saturating_add(1);
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

            let item = json!({
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
            });
            let (retained, item_truncated) =
                retain_newest(items.drain(..).chain(std::iter::once(item)), capacity);
            items = retained;
            truncated |= item_truncated;
        }
    }
    Ok(page_source_items(items, truncated, offset))
}

fn workflow_terminal_items(offset: usize) -> Result<(Vec<Value>, bool), CliError> {
    let Some(runs_root) = wardian_core::paths::workflow_runs_dir() else {
        return Ok((Vec::new(), false));
    };
    if !runs_root.exists() {
        return Ok((Vec::new(), false));
    }

    let mut items = Vec::new();
    let mut truncated = false;
    let capacity = offset
        .saturating_add(MAX_INBOX_SOURCE_ITEMS)
        .saturating_add(1);
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
            let status = match state.status {
                RunStatus::Completed => "completed",
                RunStatus::Failed => "failed",
                RunStatus::Running | RunStatus::AwaitingApproval => continue,
            };
            let events = wardian_core::engine::store::read_events(&run_dir).unwrap_or_default();
            let summary = events.iter().rev().find_map(|event| match &event.kind {
                EventKind::NodeCompleted { output, .. } => output
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                _ => None,
            });
            let updated_at = events.last().map(|event| event.ts.as_str());
            let workflow_name = wardian_core::workflow::resolve_blueprint_path(&state.blueprint_id)
                .and_then(|path| wardian_core::workflow::parse_file(&path).ok())
                .map(|blueprint| blueprint.name)
                .unwrap_or_else(|| state.blueprint_id.clone());

            let item = json!({
                "id": format!("workflow-completion:{}:{}", state.blueprint_id, state.run_id),
                "type": "workflow_completed",
                "timestamp": updated_at.map(timestamp_millis).unwrap_or_default(),
                "read": false,
                "evidence_source": "live_runtime",
                "workflow_id": state.blueprint_id,
                "workflow_run_id": state.run_id,
                "workflow_name": workflow_name,
                "status": status,
                "error": state.failure,
                "summary": summary,
            });
            let (retained, item_truncated) =
                retain_newest(items.drain(..).chain(std::iter::once(item)), capacity);
            items = retained;
            truncated |= item_truncated;
        }
    }
    Ok(page_source_items(items, truncated, offset))
}

fn notification_items(
    records: &[InteractionRecord],
    read_notification_ids: &HashSet<String>,
    agent_names: &HashMap<String, String>,
    decisions: &HashMap<String, InboxNotificationDecision>,
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
            if let Some(decision) = decisions.get(notification_id) {
                item["approval_decision"] = Value::String(decision.choice.clone());
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
        let output = render_list(&InboxProjection {
            items,
            truncated: true,
            next_offset: Some(1),
        })
        .unwrap();
        let output: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["items"][0]["id"], "new");
        assert_eq!(output["truncated"], true);
        assert_eq!(output["next_offset"], 1);
    }

    #[test]
    fn merged_pagination_filters_before_advancing_the_global_cursor() {
        let mut pages = vec![
            InboxSourcePage {
                items: vec![
                    json!({
                        "id": "read-new",
                        "timestamp": 4,
                        "read": true,
                        "type": "agent_update",
                        "evidence_source": "interaction_store"
                    }),
                    json!({
                        "id": "unread-new",
                        "timestamp": 3,
                        "read": false,
                        "type": "agent_update",
                        "evidence_source": "interaction_store"
                    }),
                ],
                truncated: false,
            },
            InboxSourcePage {
                items: vec![json!({
                    "id": "unread-old",
                    "timestamp": 2,
                    "read": false,
                    "type": "agent_completed",
                    "evidence_source": "provider_runtime"
                })],
                truncated: false,
            },
        ];
        let types = HashSet::new();
        let sources = HashSet::new();
        let request = InboxPageRequest {
            offset: 1,
            limit: 1,
            types: &types,
            sources: &sources,
            unread: true,
        };
        let mut source_offsets = [0; 2];
        let (items, truncated, next_offset) = merge_persisted_pages(
            &request,
            &mut pages,
            &mut |_index, _offset| unreachable!("fixture has no continuation"),
            &mut source_offsets,
        )
        .unwrap();

        assert_eq!(items[0]["id"], "unread-old");
        assert!(!truncated);
        assert_eq!(next_offset, None);
    }

    #[test]
    fn merged_pagination_continues_across_sources_before_returning_a_cursor() {
        let mut pages = vec![
            InboxSourcePage {
                items: vec![json!({"id": "notification", "timestamp": 3})],
                truncated: false,
            },
            InboxSourcePage {
                items: vec![
                    json!({"id": "queue-new", "timestamp": 2}),
                    json!({"id": "queue-old", "timestamp": 1}),
                ],
                truncated: false,
            },
        ];
        let types = HashSet::new();
        let sources = HashSet::new();
        let request = InboxPageRequest {
            offset: 1,
            limit: 1,
            types: &types,
            sources: &sources,
            unread: false,
        };
        let mut source_offsets = [0; 2];
        let (items, truncated, next_offset) = merge_persisted_pages(
            &request,
            &mut pages,
            &mut |_index, _offset| unreachable!("fixture has no continuation"),
            &mut source_offsets,
        )
        .unwrap();

        assert_eq!(items[0]["id"], "queue-new");
        assert!(truncated);
        assert_eq!(next_offset, Some(2));
    }

    #[test]
    fn merged_pagination_refills_an_exhausted_source_before_selecting_next_head() {
        let mut pages = vec![
            InboxSourcePage {
                items: vec![json!({"id": "first", "timestamp": 3})],
                truncated: true,
            },
            InboxSourcePage {
                items: vec![json!({"id": "other", "timestamp": 1})],
                truncated: false,
            },
        ];
        let types = HashSet::new();
        let sources = HashSet::new();
        let request = InboxPageRequest {
            offset: 0,
            limit: 2,
            types: &types,
            sources: &sources,
            unread: false,
        };
        let mut source_offsets = [0; 2];
        let (items, truncated, next_offset) = merge_persisted_pages(
            &request,
            &mut pages,
            &mut |index, offset| {
                assert_eq!(index, 0);
                assert_eq!(offset, MAX_INBOX_SOURCE_ITEMS);
                Ok(InboxSourcePage {
                    items: vec![json!({"id": "refilled", "timestamp": 2})],
                    truncated: false,
                })
            },
            &mut source_offsets,
        )
        .unwrap();

        assert_eq!(
            items
                .iter()
                .map(|item| item["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["first", "refilled"]
        );
        assert!(truncated);
        assert_eq!(next_offset, Some(2));
    }
}

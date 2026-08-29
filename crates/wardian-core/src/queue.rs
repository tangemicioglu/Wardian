use serde::de::{SeqAccess, Visitor};
use serde::Deserializer;
use serde_json::Value;
use std::{collections::HashSet, fs::File, io::BufReader, path::Path};

/// The bounded visible portion of the persisted Inbox queue plus the
/// bookkeeping needed to reconcile durable projections.
#[derive(Debug, Default)]
pub struct QueueProjection {
    pub items: Vec<Value>,
    pub read_notification_ids: HashSet<String>,
    pub automation_runs: HashSet<(String, String)>,
    pub truncated: bool,
}

/// Streams a persisted queue array and returns one bounded page of visible
/// items. Read acknowledgements and automation dismissal markers are retained as
/// separate metadata so they cannot evict visible events or expire with the
/// seven-day legacy queue retention window.
pub fn read_recent_items(path: &Path, limit: usize, offset: usize, cutoff: i64) -> QueueProjection {
    read_recent_items_matching(path, limit, offset, cutoff, |_| true)
}

/// Streams a persisted queue page while retaining only visible items that
/// satisfy `matches`. Queue metadata is still collected for every item so a
/// filtered read cannot lose durable acknowledgements or automation identities.
pub fn read_recent_items_matching<F>(
    path: &Path,
    limit: usize,
    offset: usize,
    cutoff: i64,
    matches: F,
) -> QueueProjection
where
    F: Fn(&Value) -> bool,
{
    let Ok(file) = File::open(path) else {
        return QueueProjection::default();
    };
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(file));
    deserializer
        .deserialize_seq(QueueVisitor {
            limit,
            offset,
            cutoff,
            matches,
        })
        .unwrap_or_default()
}

/// Streams the persisted queue for the current Wardian home.
pub fn load_recent_items(limit: usize, offset: usize, cutoff: i64) -> QueueProjection {
    load_recent_items_matching(limit, offset, cutoff, |_| true)
}

/// Streams the current Wardian queue with a visible-item predicate.
pub fn load_recent_items_matching<F>(
    limit: usize,
    offset: usize,
    cutoff: i64,
    matches: F,
) -> QueueProjection
where
    F: Fn(&Value) -> bool,
{
    let Some(path) = crate::paths::wardian_home().map(|home| home.join("queue/items.json")) else {
        return QueueProjection::default();
    };
    read_recent_items_matching(&path, limit, offset, cutoff, matches)
}

struct QueueVisitor<F> {
    limit: usize,
    offset: usize,
    cutoff: i64,
    matches: F,
}

impl<'de, F> Visitor<'de> for QueueVisitor<F>
where
    F: Fn(&Value) -> bool,
{
    type Value = QueueProjection;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an Inbox queue array")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let capacity = self.offset.saturating_add(self.limit).saturating_add(1);
        let mut visible = Vec::new();
        let mut projection = QueueProjection::default();

        while let Some(item) = sequence.next_element::<Value>()? {
            if let Some(notification_id) = read_acknowledgement_id(&item) {
                projection.read_notification_ids.insert(notification_id);
                continue;
            }
            let item = normalize_legacy_item(item);
            let dismissed = item.get("dismissed").and_then(Value::as_bool) == Some(true);
            if let Some(automation_run) = automation_run_identity(&item) {
                // A dismissal marker is durable triage metadata. Ordinary
                // automation completions follow the legacy queue retention
                // window, so an expired completion must not suppress a fresh
                // checkpoint projection.
                if dismissed || is_recent(&item, self.cutoff) {
                    projection.automation_runs.insert(automation_run);
                }
            }
            if dismissed || item.get("automation_approval").is_some() {
                continue;
            }
            if !is_recent(&item, self.cutoff) {
                continue;
            }
            if !(self.matches)(&item) {
                continue;
            }

            visible.push(item);
            visible.sort_by(compare_items);
            if visible.len() > capacity {
                visible.pop();
                projection.truncated = true;
            }
        }

        projection.truncated |= visible.len() > self.offset.saturating_add(self.limit);
        projection.items = visible
            .into_iter()
            .skip(self.offset)
            .take(self.limit)
            .collect();
        Ok(projection)
    }
}

fn read_acknowledgement_id(item: &Value) -> Option<String> {
    (item.get("type").and_then(Value::as_str) == Some("agent_update")
        && item.get("read").and_then(Value::as_bool) == Some(true))
    .then(|| item.get("inbox_notification_id").and_then(Value::as_str))
    .flatten()
    .map(ToString::to_string)
}

fn automation_run_identity(item: &Value) -> Option<(String, String)> {
    matches!(
        item.get("type").and_then(Value::as_str),
        Some("automation_completed" | "workflow_completed")
    )
    .then(|| {
        Some((
            item.get("automation_id")
                .or_else(|| item.get("workflow_id"))
                .and_then(Value::as_str)?
                .to_string(),
            item.get("automation_run_id")
                .or_else(|| item.get("workflow_run_id"))
                .and_then(Value::as_str)?
                .to_string(),
        ))
    })
    .flatten()
}

fn normalize_legacy_item(mut item: Value) -> Value {
    let Some(object) = item.as_object_mut() else {
        return item;
    };
    match object.get("type").and_then(Value::as_str) {
        Some("workflow_completed") => {
            object.insert(
                "type".to_string(),
                Value::String("automation_completed".to_string()),
            );
        }
        Some("workflow_failed") => {
            object.insert(
                "type".to_string(),
                Value::String("automation_completed".to_string()),
            );
            object
                .entry("status".to_string())
                .or_insert_with(|| Value::String("failed".to_string()));
        }
        _ => {}
    }
    for (legacy_key, canonical_key) in [
        ("workflow_id", "automation_id"),
        ("workflow_run_id", "automation_run_id"),
        ("workflow_name", "automation_name"),
        ("workflow_approval", "automation_approval"),
    ] {
        if let Some(value) = object.remove(legacy_key) {
            object.entry(canonical_key.to_string()).or_insert(value);
        }
    }
    item
}

fn is_recent(item: &Value, cutoff: i64) -> bool {
    item_timestamp(item).is_some_and(|timestamp| timestamp > cutoff)
}

fn item_timestamp(item: &Value) -> Option<i64> {
    item.get("timestamp").and_then(Value::as_i64).or_else(|| {
        item.get("timestamp")
            .and_then(Value::as_u64)
            .and_then(|value| i64::try_from(value).ok())
    })
}

fn compare_items(left: &Value, right: &Value) -> std::cmp::Ordering {
    item_timestamp(right)
        .unwrap_or_default()
        .cmp(&item_timestamp(left).unwrap_or_default())
        .then_with(|| {
            right
                .get("id")
                .and_then(Value::as_str)
                .cmp(&left.get("id").and_then(Value::as_str))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn pages_visible_items_without_capping_triage_metadata() {
        let file = tempfile::NamedTempFile::new().expect("queue fixture");
        fs::write(
            file.path(),
            serde_json::json!([
                { "id": "old-visible", "type": "agent_completed", "timestamp": 1 },
                { "id": "new-visible", "type": "action_needed", "timestamp": 3 },
                { "id": "missing-timestamp", "type": "agent_completed" },
                { "id": "string-timestamp", "type": "agent_completed", "timestamp": "3" },
                { "id": "read-ack", "type": "agent_update", "read": true, "inbox_notification_id": "notification-1", "timestamp": 0 },
                { "id": "dismissed", "type": "workflow_completed", "workflow_id": "deploy", "workflow_run_id": "run-1", "dismissed": true, "timestamp": 0 },
                { "id": "expired-workflow", "type": "workflow_completed", "workflow_id": "deploy", "workflow_run_id": "run-2", "timestamp": -1 }
            ])
            .to_string(),
        )
        .expect("write queue fixture");

        let first = read_recent_items(file.path(), 1, 0, 0);
        assert_eq!(first.items[0]["id"], "new-visible");
        assert!(first.truncated);
        let all = read_recent_items(file.path(), 10, 0, 0);
        assert_eq!(
            all.items
                .iter()
                .map(|item| item["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["new-visible", "old-visible"]
        );
        assert!(first.read_notification_ids.contains("notification-1"));
        assert!(first
            .automation_runs
            .contains(&("deploy".to_string(), "run-1".to_string())));
        assert!(!first
            .automation_runs
            .contains(&("deploy".to_string(), "run-2".to_string())));

        let second = read_recent_items(file.path(), 1, 1, 0);
        assert_eq!(second.items[0]["id"], "old-visible");
        assert!(!second.truncated);
    }
}

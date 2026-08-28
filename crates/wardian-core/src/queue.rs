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
    pub workflow_runs: HashSet<(String, String)>,
    pub truncated: bool,
}

/// Streams a persisted queue array and returns one bounded page of visible
/// items. Read acknowledgements and workflow dismissal markers are retained as
/// separate metadata so they cannot evict visible events or expire with the
/// seven-day legacy queue retention window.
pub fn read_recent_items(path: &Path, limit: usize, offset: usize, cutoff: i64) -> QueueProjection {
    let Ok(file) = File::open(path) else {
        return QueueProjection::default();
    };
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(file));
    deserializer
        .deserialize_seq(QueueVisitor {
            limit,
            offset,
            cutoff,
        })
        .unwrap_or_default()
}

/// Streams the persisted queue for the current Wardian home.
pub fn load_recent_items(limit: usize, offset: usize, cutoff: i64) -> QueueProjection {
    let Some(path) = crate::paths::wardian_home().map(|home| home.join("queue/items.json")) else {
        return QueueProjection::default();
    };
    read_recent_items(&path, limit, offset, cutoff)
}

struct QueueVisitor {
    limit: usize,
    offset: usize,
    cutoff: i64,
}

impl<'de> Visitor<'de> for QueueVisitor {
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
            if let Some(workflow_run) = workflow_run_identity(&item) {
                projection.workflow_runs.insert(workflow_run);
            }
            if item.get("dismissed").and_then(Value::as_bool) == Some(true)
                || item.get("workflow_approval").is_some()
            {
                continue;
            }
            if !is_recent(&item, self.cutoff) {
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

fn workflow_run_identity(item: &Value) -> Option<(String, String)> {
    (item.get("type").and_then(Value::as_str) == Some("workflow_completed"))
        .then(|| {
            Some((
                item.get("workflow_id").and_then(Value::as_str)?.to_string(),
                item.get("workflow_run_id")
                    .and_then(Value::as_str)?
                    .to_string(),
            ))
        })
        .flatten()
}

fn is_recent(item: &Value, cutoff: i64) -> bool {
    item_timestamp(item).is_none_or(|timestamp| timestamp > cutoff)
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
                { "id": "read-ack", "type": "agent_update", "read": true, "inbox_notification_id": "notification-1", "timestamp": 0 },
                { "id": "dismissed", "type": "workflow_completed", "workflow_id": "deploy", "workflow_run_id": "run-1", "dismissed": true, "timestamp": 0 }
            ])
            .to_string(),
        )
        .expect("write queue fixture");

        let first = read_recent_items(file.path(), 1, 0, 0);
        assert_eq!(first.items[0]["id"], "new-visible");
        assert!(first.truncated);
        assert!(first.read_notification_ids.contains("notification-1"));
        assert!(first
            .workflow_runs
            .contains(&("deploy".to_string(), "run-1".to_string())));

        let second = read_recent_items(file.path(), 1, 1, 0);
        assert_eq!(second.items[0]["id"], "old-visible");
        assert!(!second.truncated);
    }
}

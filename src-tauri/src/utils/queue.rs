use serde_json::Value;
use std::collections::HashSet;

pub fn load_items() -> Vec<Value> {
    crate::utils::fs::get_wardian_home()
        .and_then(|home| std::fs::read_to_string(home.join("queue").join("items.json")).ok())
        .and_then(|data| serde_json::from_str::<Vec<Value>>(&data).ok())
        .unwrap_or_default()
}

pub fn save_items(items: &[Value]) -> Result<(), String> {
    let home = crate::utils::fs::get_wardian_home().ok_or_else(|| "no wardian home".to_string())?;
    let queue_dir = home.join("queue");
    std::fs::create_dir_all(&queue_dir).map_err(|error| error.to_string())?;
    wardian_core::conversations::write_json_atomic(&queue_dir.join("items.json"), items)
        .map_err(|error| error.to_string())
}

/// Merges a desktop queue snapshot with the latest persisted projection.
///
/// The desktop queue historically sends a complete snapshot after each local
/// mutation. When a remote mutation lands after the desktop load, the base
/// snapshot lets us distinguish desktop removals from items added remotely.
/// Read state is monotonic, so a remote read acknowledgement wins conflicts.
pub fn merge_desktop_snapshot(
    base: Option<&[Value]>,
    incoming: &[Value],
    latest: &[Value],
) -> Vec<Value> {
    let Some(base) = base else {
        return merge_latest_and_incoming(incoming, latest);
    };
    let base_ids = base.iter().filter_map(item_id).collect::<HashSet<_>>();
    let incoming_by_id = incoming
        .iter()
        .filter_map(|item| item_id(item).map(|id| (id, item)))
        .collect::<std::collections::HashMap<_, _>>();
    let latest_ids = latest.iter().filter_map(item_id).collect::<HashSet<_>>();
    let mut merged = Vec::with_capacity(latest.len() + incoming.len());

    for latest_item in latest {
        let Some(id) = item_id(latest_item) else {
            merged.push(latest_item.clone());
            continue;
        };
        if !base_ids.contains(id) {
            merged.push(latest_item.clone());
        } else if let Some(incoming_item) = incoming_by_id.get(id) {
            let mut item = (*incoming_item).clone();
            if latest_item.get("read").and_then(Value::as_bool) == Some(true) {
                item["read"] = Value::Bool(true);
            }
            merged.push(item);
        }
    }

    for incoming_item in incoming {
        let Some(id) = item_id(incoming_item) else {
            merged.push(incoming_item.clone());
            continue;
        };
        if !base_ids.contains(id) && !latest_ids.contains(id) {
            merged.push(incoming_item.clone());
        }
    }
    merged
}

fn merge_latest_and_incoming(incoming: &[Value], latest: &[Value]) -> Vec<Value> {
    let incoming_by_id = incoming
        .iter()
        .filter_map(|item| item_id(item).map(|id| (id, item)))
        .collect::<std::collections::HashMap<_, _>>();
    let latest_ids = latest.iter().filter_map(item_id).collect::<HashSet<_>>();
    let mut merged = latest
        .iter()
        .map(|latest_item| {
            let Some(id) = item_id(latest_item) else {
                return latest_item.clone();
            };
            let Some(incoming_item) = incoming_by_id.get(id) else {
                return latest_item.clone();
            };
            let mut item = (*incoming_item).clone();
            if latest_item.get("read").and_then(Value::as_bool) == Some(true) {
                item["read"] = Value::Bool(true);
            }
            item
        })
        .collect::<Vec<_>>();
    merged.extend(incoming.iter().filter_map(|item| {
        let id = item_id(item)?;
        (!latest_ids.contains(id)).then(|| item.clone())
    }));
    merged
}

fn item_id(item: &Value) -> Option<&str> {
    item.get("id").and_then(Value::as_str)
}

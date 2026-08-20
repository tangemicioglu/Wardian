use serde_json::Value;

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

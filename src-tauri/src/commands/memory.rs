use wardian_core::memory::{
    MemoryRecord, MemoryStore, RecallResult, SaveMemoryRequest, UpdateMemoryRequest,
};

#[tauri::command]
pub async fn memory_save(request: SaveMemoryRequest) -> Result<MemoryRecord, String> {
    MemoryStore::from_default_home()
        .and_then(|store| store.save(request))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn memory_list(
    agent_id: String,
    workspace: Option<String>,
) -> Result<Vec<MemoryRecord>, String> {
    MemoryStore::from_default_home()
        .and_then(|store| store.list_active(&agent_id, workspace.as_deref()))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn memory_get(memory_id: String) -> Result<MemoryRecord, String> {
    MemoryStore::from_default_home()
        .and_then(|store| store.get(&memory_id))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn memory_update(request: UpdateMemoryRequest) -> Result<MemoryRecord, String> {
    MemoryStore::from_default_home()
        .and_then(|store| store.update(request))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn memory_remove(memory_id: String) -> Result<MemoryRecord, String> {
    MemoryStore::from_default_home()
        .and_then(|store| store.remove(&memory_id))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn memory_recall(
    agent_id: String,
    workspace: Option<String>,
) -> Result<RecallResult, String> {
    MemoryStore::from_default_home()
        .and_then(|store| store.recall(&agent_id, workspace.as_deref()))
        .map_err(|error| error.to_string())
}

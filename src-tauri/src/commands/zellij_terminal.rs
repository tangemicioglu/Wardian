use crate::state::zellij_terminal::ZellijPanePhase;
use crate::state::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct ZellijTerminalPreview {
    pub session_id: String,
    pub terminal_session_id: String,
    pub generation: Option<u64>,
    pub state: &'static str,
    pub content: String,
}

#[tauri::command]
pub async fn get_zellij_terminal_preview(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<ZellijTerminalPreview, String> {
    let Some(engine) = state.zellij_terminal.get() else {
        return Ok(ZellijTerminalPreview {
            terminal_session_id: session_id.clone(),
            session_id,
            generation: None,
            state: "unavailable",
            content: String::new(),
        });
    };
    let Some(binding) = engine.binding(&session_id).await else {
        return Ok(ZellijTerminalPreview {
            terminal_session_id: session_id.clone(),
            session_id,
            generation: None,
            state: "starting",
            content: String::new(),
        });
    };
    if binding.phase == ZellijPanePhase::Exited {
        return Ok(ZellijTerminalPreview {
            terminal_session_id: session_id.clone(),
            session_id,
            generation: Some(binding.generation),
            state: "exited",
            content: String::new(),
        });
    }
    let content = state
        .terminal_sessions
        .snapshot(&session_id)
        .await
        .map(|snapshot| snapshot.visible_grid)
        .unwrap_or_default();
    Ok(ZellijTerminalPreview {
        terminal_session_id: session_id.clone(),
        session_id,
        generation: Some(binding.generation),
        state: "running",
        content,
    })
}

#[tauri::command]
pub async fn activate_zellij_agent_terminal(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let engine = state
        .zellij_terminal
        .get()
        .ok_or_else(|| "Terminal engine is unavailable".to_string())?;
    engine.start_attached_client().await?;
    let binding = engine
        .binding(&session_id)
        .await
        .ok_or_else(|| "Agent terminal is still starting".to_string())?;
    engine
        .activate_pane(&session_id, binding.generation)
        .await?;
    Ok(session_id)
}

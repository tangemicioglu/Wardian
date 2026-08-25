use crate::state::zellij_terminal::{ZellijPanePhase, HABITAT_TERMINAL_SESSION_ID};
use crate::state::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct ZellijTerminalPreview {
    pub session_id: String,
    pub habitat_terminal_session_id: &'static str,
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
            session_id,
            habitat_terminal_session_id: HABITAT_TERMINAL_SESSION_ID,
            generation: None,
            state: "unavailable",
            content: String::new(),
        });
    };
    let Some(binding) = engine.binding(&session_id).await else {
        return Ok(ZellijTerminalPreview {
            session_id,
            habitat_terminal_session_id: HABITAT_TERMINAL_SESSION_ID,
            generation: None,
            state: "starting",
            content: String::new(),
        });
    };
    if binding.phase == ZellijPanePhase::Exited {
        return Ok(ZellijTerminalPreview {
            session_id,
            habitat_terminal_session_id: HABITAT_TERMINAL_SESSION_ID,
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
        session_id,
        habitat_terminal_session_id: HABITAT_TERMINAL_SESSION_ID,
        generation: Some(binding.generation),
        state: "running",
        content,
    })
}

#[tauri::command]
pub async fn activate_zellij_agent_terminal(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<&'static str, String> {
    let engine = state
        .zellij_terminal
        .get()
        .ok_or_else(|| "Terminal engine is unavailable".to_string())?;
    engine
        .start_attached_client(
            state.terminal_sessions.clone(),
            wardian_core::models::TerminalGeometry {
                cols: 120,
                rows: 40,
            },
        )
        .await?;
    let binding = engine
        .binding(&session_id)
        .await
        .ok_or_else(|| "Agent terminal is still starting".to_string())?;
    engine
        .activate_pane(&session_id, binding.generation)
        .await?;
    Ok(HABITAT_TERMINAL_SESSION_ID)
}

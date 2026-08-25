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

fn preview_state_for_phase(phase: ZellijPanePhase) -> &'static str {
    match phase {
        ZellijPanePhase::Starting | ZellijPanePhase::Closing => "starting",
        ZellijPanePhase::Running => "running",
        ZellijPanePhase::Exited => "exited",
    }
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
    let preview_state = preview_state_for_phase(binding.phase);
    if preview_state != "running" {
        return Ok(ZellijTerminalPreview {
            terminal_session_id: session_id.clone(),
            session_id,
            generation: Some(binding.generation),
            state: preview_state,
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
        state: preview_state,
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_phase_mapping_only_advertises_running_panes_as_interactive() {
        assert_eq!(
            preview_state_for_phase(ZellijPanePhase::Starting),
            "starting"
        );
        assert_eq!(
            preview_state_for_phase(ZellijPanePhase::Closing),
            "starting"
        );
        assert_eq!(preview_state_for_phase(ZellijPanePhase::Running), "running");
        assert_eq!(preview_state_for_phase(ZellijPanePhase::Exited), "exited");
    }
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

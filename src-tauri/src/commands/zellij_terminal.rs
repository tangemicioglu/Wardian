use crate::state::zellij_terminal::ZellijPanePhase;
use crate::state::AppState;
use serde::Serialize;
use tauri::State;

const HABITAT_TERMINAL_PRESENTATION_ID: &str = "desktop:zellij-habitat-terminal";

#[derive(Debug, Serialize)]
pub struct ZellijTerminalPreview {
    pub session_id: String,
    pub terminal_session_id: String,
    pub generation: Option<u64>,
    pub broker_generation: Option<u64>,
    pub broker_lease_epoch: Option<u64>,
    pub broker_owner_presentation_id: Option<String>,
    pub broker_activation_pending: bool,
    pub state: &'static str,
    pub content: String,
}

fn preview_state_for_phase(
    phase: ZellijPanePhase,
    broker_available: bool,
    replacement_pending: bool,
) -> &'static str {
    if replacement_pending {
        return "starting";
    }
    match phase {
        ZellijPanePhase::Starting | ZellijPanePhase::Closing => "starting",
        ZellijPanePhase::Running if broker_available => "running",
        ZellijPanePhase::Running => "starting",
        ZellijPanePhase::Exited => "exited",
    }
}

fn preview_state_without_binding(status: Option<&str>, has_runtime: bool) -> &'static str {
    if !has_runtime
        && status.is_some_and(|value| {
            wardian_core::identity::normalize_status(value).as_str() == "error"
        })
    {
        "error"
    } else {
        "starting"
    }
}

#[tauri::command]
pub async fn get_zellij_terminal_preview(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<ZellijTerminalPreview, String> {
    let broker_state = state.terminal_sessions.broker_state(&session_id).await.ok();
    let broker_generation = broker_state
        .as_ref()
        .map(|broker| broker.runtime_generation);
    let broker_lease_epoch = broker_state.as_ref().map(|broker| broker.lease_epoch);
    let broker_owner_presentation_id = broker_state
        .as_ref()
        .and_then(|broker| broker.owner_presentation_id.clone());
    let broker_activation_pending = broker_state
        .as_ref()
        .is_some_and(|broker| broker.pending_activation.is_some());
    let Some(engine) = state.zellij_terminal.get() else {
        return Ok(ZellijTerminalPreview {
            terminal_session_id: session_id.clone(),
            session_id,
            generation: None,
            broker_generation,
            broker_lease_epoch,
            broker_owner_presentation_id,
            broker_activation_pending,
            state: "unavailable",
            content: String::new(),
        });
    };
    let replacement_pending = engine.replacement_pending(&session_id);
    let Some(binding) = engine.binding(&session_id).await else {
        let preview_state = {
            let agents = state.agents.lock().await;
            agents.get(&session_id).map_or("starting", |agent| {
                let status = agent
                    .current_status
                    .lock()
                    .map(|value| value.clone())
                    .unwrap_or_default();
                preview_state_without_binding(
                    Some(&status),
                    agent.runtime_generation.is_some(),
                )
            })
        };
        return Ok(ZellijTerminalPreview {
            terminal_session_id: session_id.clone(),
            session_id,
            generation: None,
            broker_generation,
            broker_lease_epoch,
            broker_owner_presentation_id,
            broker_activation_pending,
            state: preview_state,
            content: String::new(),
        });
    };
    let preview_state = preview_state_for_phase(
        binding.phase,
        broker_state.is_some(),
        replacement_pending,
    );
    if preview_state != "running" {
        return Ok(ZellijTerminalPreview {
            terminal_session_id: session_id.clone(),
            session_id,
            generation: Some(binding.generation),
            broker_generation,
            broker_lease_epoch,
            broker_owner_presentation_id,
            broker_activation_pending,
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
        broker_generation,
        broker_lease_epoch,
        broker_owner_presentation_id,
        broker_activation_pending,
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
            preview_state_for_phase(ZellijPanePhase::Starting, true, false),
            "starting"
        );
        assert_eq!(
            preview_state_for_phase(ZellijPanePhase::Closing, true, false),
            "starting"
        );
        assert_eq!(
            preview_state_for_phase(ZellijPanePhase::Running, true, false),
            "running"
        );
        assert_eq!(
            preview_state_for_phase(ZellijPanePhase::Running, false, false),
            "starting"
        );
        assert_eq!(
            preview_state_for_phase(ZellijPanePhase::Exited, true, false),
            "exited"
        );
        assert_eq!(
            preview_state_for_phase(ZellijPanePhase::Running, true, true),
            "starting"
        );
    }

    #[test]
    fn missing_binding_exposes_a_runtime_less_restart_failure() {
        assert_eq!(preview_state_without_binding(Some("Error"), false), "error");
        assert_eq!(preview_state_without_binding(Some("Starting"), false), "starting");
        assert_eq!(preview_state_without_binding(Some("Error"), true), "starting");
    }

}

#[tauri::command]
pub async fn activate_zellij_agent_terminal(
    session_id: String,
    broker_generation: Option<u64>,
    broker_lease_epoch: Option<u64>,
    activation_request_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let engine = state
        .zellij_terminal
        .get()
        .ok_or_else(|| "Terminal engine is unavailable".to_string())?;
    engine.register_activation_request(&activation_request_id);
    if engine.replacement_pending(&session_id) {
        return Err("Agent terminal restart is still settling".to_string());
    }
    let binding = engine
        .binding(&session_id)
        .await
        .ok_or_else(|| "Agent terminal is still starting".to_string())?;
    engine.start_attached_client().await?;
    let broker_generation = broker_generation
        .ok_or_else(|| "Agent terminal ownership state is still loading".to_string())?;
    let broker_lease_epoch = broker_lease_epoch
        .ok_or_else(|| "Agent terminal ownership state is still loading".to_string())?;
    let focus_engine = engine.clone();
    let focus_session_id = session_id.clone();
    let focus_request_id = activation_request_id.clone();
    state
        .terminal_sessions
        .run_authorized_native_focus(
            &session_id,
            broker_generation,
            broker_lease_epoch,
            HABITAT_TERMINAL_PRESENTATION_ID,
            move || async move {
                focus_engine
                    .activate_pane_for_request(
                        &focus_session_id,
                        binding.generation,
                        &focus_request_id,
                    )
                    .await
            },
        )
        .await
        .map_err(|_| "Agent terminal ownership changed; retry the selection".to_string())?;
    Ok(session_id)
}

#[tauri::command]
pub async fn cancel_zellij_agent_terminal_activation(
    activation_request_id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let Some(engine) = state.zellij_terminal.get() else {
        return Ok(false);
    };
    Ok(engine.cancel_activation_request(&activation_request_id))
}

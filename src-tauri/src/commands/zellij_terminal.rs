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

fn preview_state_for_phase(phase: ZellijPanePhase) -> &'static str {
    match phase {
        ZellijPanePhase::Starting | ZellijPanePhase::Closing => "starting",
        ZellijPanePhase::Running => "running",
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

fn validate_habitat_activation(
    broker_generation: u64,
    observed_broker_generation: Option<u64>,
    broker_owner: Option<&str>,
    broker_activation_pending: bool,
) -> Result<(), String> {
    if observed_broker_generation.is_some_and(|observed| observed != broker_generation) {
        return Err("Agent terminal generation changed; retry the selection".to_string());
    }
    if broker_owner.is_some_and(|owner| owner != HABITAT_TERMINAL_PRESENTATION_ID) {
        return Err("Agent terminal is currently controlled from another presentation".to_string());
    }
    if broker_activation_pending {
        return Err(
            "Agent terminal ownership transfer is in progress; retry the selection".to_string(),
        );
    }
    Ok(())
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
    let preview_state = preview_state_for_phase(binding.phase);
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

    #[test]
    fn missing_binding_exposes_a_runtime_less_restart_failure() {
        assert_eq!(preview_state_without_binding(Some("Error"), false), "error");
        assert_eq!(preview_state_without_binding(Some("Starting"), false), "starting");
        assert_eq!(preview_state_without_binding(Some("Error"), true), "starting");
    }

    #[test]
    fn habitat_activation_rejects_foreign_owners_and_stale_generations() {
        assert!(validate_habitat_activation(4, None, None, false).is_ok());
        assert!(validate_habitat_activation(
            4,
            Some(4),
            Some(HABITAT_TERMINAL_PRESENTATION_ID),
            false
        )
        .is_ok());
        assert!(validate_habitat_activation(4, Some(3), None, false).is_err());
        assert!(
            validate_habitat_activation(4, Some(4), Some("remote:paired-device"), false).is_err()
        );
        assert!(validate_habitat_activation(4, Some(4), None, true).is_err());
    }
}

#[tauri::command]
pub async fn activate_zellij_agent_terminal(
    session_id: String,
    broker_generation: Option<u64>,
    activation_request_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let engine = state
        .zellij_terminal
        .get()
        .ok_or_else(|| "Terminal engine is unavailable".to_string())?;
    engine.register_activation_request(&activation_request_id);
    let binding = engine
        .binding(&session_id)
        .await
        .ok_or_else(|| "Agent terminal is still starting".to_string())?;
    engine.start_attached_client().await?;
    let broker_state = state
        .terminal_sessions
        .broker_state(&session_id)
        .await
        .map_err(|error| format!("Agent terminal ownership is unavailable: {error}"))?;
    validate_habitat_activation(
        broker_state.runtime_generation,
        broker_generation,
        broker_state.owner_presentation_id.as_deref(),
        broker_state.pending_activation.is_some(),
    )?;
    engine
        .activate_pane_for_request(
            &session_id,
            binding.generation,
            &activation_request_id,
        )
        .await?;
    Ok(session_id)
}

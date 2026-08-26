use crate::state::zellij_terminal::ZellijPanePhase;
use crate::state::AppState;
use serde::Serialize;
use tauri::State;

const HABITAT_TERMINAL_PRESENTATION_ID: &str = "desktop:zellij-habitat-terminal";
const OWNERSHIP_CHANGED_MESSAGE: &str = "Agent terminal ownership changed; retry the selection";

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

fn validate_habitat_activation_preflight(
    current_generation: u64,
    current_lease_epoch: u64,
    current_owner: Option<&str>,
    activation_pending: bool,
    observed_generation: u64,
    observed_lease_epoch: u64,
) -> Result<(), String> {
    if current_generation != observed_generation
        || current_lease_epoch != observed_lease_epoch
        || activation_pending
        || current_owner.is_some_and(|owner| owner != HABITAT_TERMINAL_PRESENTATION_ID)
    {
        return Err(OWNERSHIP_CHANGED_MESSAGE.to_string());
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
                preview_state_without_binding(Some(&status), agent.runtime_generation.is_some())
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
    let preview_state =
        preview_state_for_phase(binding.phase, broker_state.is_some(), replacement_pending);
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
        assert_eq!(
            preview_state_without_binding(Some("Starting"), false),
            "starting"
        );
        assert_eq!(
            preview_state_without_binding(Some("Error"), true),
            "starting"
        );
    }

    #[test]
    fn habitat_activation_preflight_rejects_changed_or_pending_ownership() {
        for result in [
            validate_habitat_activation_preflight(7, 11, None, true, 7, 11),
            validate_habitat_activation_preflight(7, 11, Some("remote:client"), false, 7, 11),
            validate_habitat_activation_preflight(8, 11, None, false, 7, 11),
            validate_habitat_activation_preflight(7, 12, None, false, 7, 11),
        ] {
            assert_eq!(result.unwrap_err(), OWNERSHIP_CHANGED_MESSAGE);
        }
    }

    #[test]
    fn habitat_activation_preflight_accepts_current_desktop_eligibility() {
        assert!(validate_habitat_activation_preflight(7, 11, None, false, 7, 11).is_ok());
        assert!(validate_habitat_activation_preflight(
            7,
            11,
            Some(HABITAT_TERMINAL_PRESENTATION_ID),
            false,
            7,
            11,
        )
        .is_ok());
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
    let broker_generation = broker_generation
        .ok_or_else(|| "Agent terminal ownership state is still loading".to_string())?;
    let broker_lease_epoch = broker_lease_epoch
        .ok_or_else(|| "Agent terminal ownership state is still loading".to_string())?;
    let broker_state = state
        .terminal_sessions
        .broker_state(&session_id)
        .await
        .map_err(|_| "Agent terminal ownership state is still loading".to_string())?;
    validate_habitat_activation_preflight(
        broker_state.runtime_generation,
        broker_state.lease_epoch,
        broker_state.owner_presentation_id.as_deref(),
        broker_state.pending_activation.is_some(),
        broker_generation,
        broker_lease_epoch,
    )?;
    engine.start_attached_client().await?;
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
        .map_err(|_| OWNERSHIP_CHANGED_MESSAGE.to_string())?;
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

use crate::state::terminal_session::TerminalSessionBroker;
use crate::state::zellij_terminal::ZellijPanePhase;
use crate::state::AppState;
use serde::Serialize;
use std::future::Future;
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

async fn run_habitat_activation<StartAttached, StartFuture, FocusAction, FocusFuture>(
    terminal_sessions: &TerminalSessionBroker,
    session_id: &str,
    observed_generation: u64,
    observed_lease_epoch: u64,
    start_attached_client: StartAttached,
    focus_action: FocusAction,
) -> Result<(), String>
where
    StartAttached: FnOnce() -> StartFuture + Send,
    StartFuture: Future<Output = Result<(), String>> + Send,
    FocusAction: FnOnce() -> FocusFuture + Send + 'static,
    FocusFuture: Future<Output = Result<(), String>> + Send + 'static,
{
    let broker_state = terminal_sessions
        .broker_state(session_id)
        .await
        .map_err(|_| "Agent terminal ownership state is still loading".to_string())?;
    validate_habitat_activation_preflight(
        broker_state.runtime_generation,
        broker_state.lease_epoch,
        broker_state.owner_presentation_id.as_deref(),
        broker_state.pending_activation.is_some(),
        observed_generation,
        observed_lease_epoch,
    )?;
    start_attached_client().await?;
    terminal_sessions
        .run_authorized_native_focus(
            session_id,
            observed_generation,
            observed_lease_epoch,
            HABITAT_TERMINAL_PRESENTATION_ID,
            focus_action,
        )
        .await
        .map_err(|_| OWNERSHIP_CHANGED_MESSAGE.to_string())
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
    use crate::state::terminal_session::{TerminalClientIdentity, TerminalRuntimeHandles};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tokio::sync::{mpsc, oneshot};
    use wardian_core::models::*;

    async fn terminal_broker_fixture() -> (Arc<TerminalSessionBroker>, u64, u64) {
        let broker = Arc::new(TerminalSessionBroker::default());
        let (input_tx, _input_rx) = mpsc::channel(16);
        let runtime = TerminalRuntimeHandles::new(input_tx, |_geometry| Ok(()));
        let generation = broker
            .start_or_replace_runtime(
                "session-1",
                runtime,
                TerminalGeometry {
                    cols: 120,
                    rows: 40,
                },
            )
            .await
            .expect("start terminal runtime");
        broker
            .register_presentation(
                terminal_registration(
                    HABITAT_TERMINAL_PRESENTATION_ID,
                    TerminalClientKind::Desktop,
                ),
                TerminalClientIdentity::trusted_desktop(),
            )
            .await
            .expect("register habitat presentation");
        let remote = broker
            .register_presentation(
                terminal_registration("remote-owner", TerminalClientKind::Remote),
                TerminalClientIdentity::authenticated_remote("remote-owner", true),
            )
            .await
            .expect("register remote presentation");
        (broker, generation, remote.broker_state.lease_epoch)
    }

    fn terminal_registration(
        presentation_id: &str,
        client_kind: TerminalClientKind,
    ) -> TerminalPresentationRegistration {
        TerminalPresentationRegistration {
            presentation_id: presentation_id.to_string(),
            session_id: "session-1".to_string(),
            client_kind,
            desired_geometry: Some(TerminalGeometry {
                cols: 120,
                rows: 40,
            }),
            visibility: TerminalVisibility::Visible,
            render_state: TerminalRenderState::Mounted,
            requested_interaction: TerminalRequestedInteraction::Interactive,
            observed_lease_epoch: 0,
        }
    }

    async fn begin_remote_activation(
        broker: &TerminalSessionBroker,
        generation: u64,
        lease_epoch: u64,
    ) -> TerminalActivationBeginResult {
        broker
            .begin_activation(TerminalActivationBeginRequest {
                session_id: "session-1".to_string(),
                presentation_id: "remote-owner".to_string(),
                runtime_generation: generation,
                observed_lease_epoch: lease_epoch,
            })
            .await
            .expect("begin remote activation")
    }

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

    #[tokio::test]
    async fn pending_remote_activation_rejects_before_attached_client_startup() {
        let (broker, generation, lease_epoch) = terminal_broker_fixture().await;
        let pending = begin_remote_activation(&broker, generation, lease_epoch).await;
        assert_eq!(
            pending.decision.status,
            TerminalLeaseDecisionStatus::Accepted
        );
        let startup_ran = Arc::new(AtomicBool::new(false));
        let observed_startup = Arc::clone(&startup_ran);
        let focus_ran = Arc::new(AtomicBool::new(false));
        let observed_focus = Arc::clone(&focus_ran);

        let result = run_habitat_activation(
            &broker,
            "session-1",
            pending.decision.runtime_generation,
            pending.decision.lease_epoch,
            move || async move {
                observed_startup.store(true, Ordering::SeqCst);
                Ok(())
            },
            move || async move {
                observed_focus.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .await;

        assert_eq!(result.unwrap_err(), OWNERSHIP_CHANGED_MESSAGE);
        assert!(!startup_ran.load(Ordering::SeqCst));
        assert!(!focus_ran.load(Ordering::SeqCst));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn remote_ack_during_startup_is_rejected_by_final_focus_authorization() {
        let (broker, generation, lease_epoch) = terminal_broker_fixture().await;
        let (startup_entered_tx, startup_entered_rx) = oneshot::channel();
        let (release_startup_tx, release_startup_rx) = oneshot::channel();
        let focus_ran = Arc::new(AtomicBool::new(false));
        let observed_focus = Arc::clone(&focus_ran);
        let activation_broker = Arc::clone(&broker);
        let activation = tokio::spawn(async move {
            run_habitat_activation(
                activation_broker.as_ref(),
                "session-1",
                generation,
                lease_epoch,
                move || async move {
                    let _ = startup_entered_tx.send(());
                    let _ = release_startup_rx.await;
                    Ok(())
                },
                move || async move {
                    observed_focus.store(true, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
        });
        startup_entered_rx.await.expect("startup entered");

        let pending = begin_remote_activation(&broker, generation, lease_epoch).await;
        assert_eq!(
            pending.decision.status,
            TerminalLeaseDecisionStatus::Accepted
        );
        let acknowledged = broker
            .ack_activation(TerminalActivationAckRequest {
                session_id: "session-1".to_string(),
                presentation_id: "remote-owner".to_string(),
                runtime_generation: pending.decision.runtime_generation,
                lease_epoch: pending.decision.lease_epoch,
                activation_id: pending.activation_id.expect("remote activation id"),
            })
            .await
            .expect("ack remote activation");
        assert_eq!(
            acknowledged.decision.status,
            TerminalLeaseDecisionStatus::Accepted
        );
        assert_eq!(
            acknowledged.broker_state.owner_presentation_id.as_deref(),
            Some("remote-owner")
        );
        let _ = release_startup_tx.send(());

        assert_eq!(
            activation.await.expect("activation task").unwrap_err(),
            OWNERSHIP_CHANGED_MESSAGE
        );
        assert!(!focus_ran.load(Ordering::SeqCst));
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
    let start_engine = engine.clone();
    let focus_engine = engine.clone();
    let focus_session_id = session_id.clone();
    let focus_request_id = activation_request_id.clone();
    run_habitat_activation(
        state.terminal_sessions.as_ref(),
        &session_id,
        broker_generation,
        broker_lease_epoch,
        move || async move { start_engine.start_attached_client().await.map(|_| ()) },
        move || async move {
            focus_engine
                .activate_pane_for_request(&focus_session_id, binding.generation, &focus_request_id)
                .await
        },
    )
    .await?;
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

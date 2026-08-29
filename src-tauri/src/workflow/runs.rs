use crate::state::AppState;
use crate::workflow::{
    resolve::AgentBinding,
    runner::{HeadlessAgentRunner, TauriHeadlessAgentRunner, TauriLiveAgentRunner},
    LiveStepExecutor,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Emitter;
use wardian_core::engine::event::{Event, EventKind};
use wardian_core::engine::store::{
    append_event, read_blueprint_snapshot, read_checkpoint, read_events, write_checkpoint,
};
use wardian_core::engine::{Engine, RunStatus};
use wardian_core::models::{
    AgentConfig, InvocationKind, WorkflowAssignments, WorkflowRoleAssignment,
};
use wardian_core::workflow::Blueprint;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRunInvocation {
    pub schema: u8,
    pub provider: String,
    pub workspace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_id: Option<String>,
    #[serde(default)]
    pub bindings: HashMap<String, String>,
    #[serde(default)]
    pub assignments: WorkflowAssignments,
    /// Authenticated agent identity allowed to receive `memory_commit`
    /// mutations for this run. It is durable so approval/resume cannot lose
    /// the original authority boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_principal: Option<String>,
}

/// A durable workflow-state change that the Inbox can project immediately.
///
/// The event is emitted only after a checkpoint has been written, so the
/// Inbox can always reload an approval gate from the run log instead of
/// maintaining a second workflow lifecycle.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkflowInboxUpdate {
    pub workflow_id: String,
    pub run_instance_id: String,
    pub workflow_name: String,
    pub status: String,
    pub error: Option<String>,
    pub summary: Option<String>,
    pub updated_at: Option<String>,
}

pub const WORKFLOW_INBOX_UPDATED_EVENT: &str = "workflow-inbox-updated";

pub fn workflow_inbox_update(
    blueprint: &Blueprint,
    run_root: &Path,
) -> Option<WorkflowInboxUpdate> {
    workflow_inbox_update_with_name(&blueprint.name, run_root)
}

pub fn workflow_inbox_update_with_name(
    workflow_name: &str,
    run_root: &Path,
) -> Option<WorkflowInboxUpdate> {
    let state = read_checkpoint(run_root).ok().flatten()?;
    let (status, error) = match state.status {
        RunStatus::AwaitingApproval => ("awaiting_approval", None),
        RunStatus::Completed => ("completed", None),
        RunStatus::Failed => ("failed", state.failure),
        RunStatus::Running => return None,
    };

    let events = read_events(run_root).unwrap_or_default();
    let summary = events.iter().rev().find_map(|event| match &event.kind {
        EventKind::NodeCompleted { output, .. } | EventKind::DecisionCompleted { output, .. } => {
            output
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        }
        _ => None,
    });
    let updated_at = events.last().map(|event| event.ts.clone());

    Some(WorkflowInboxUpdate {
        workflow_id: state.blueprint_id,
        run_instance_id: state.run_id,
        workflow_name: workflow_name.to_string(),
        status: status.to_string(),
        error,
        summary,
        updated_at,
    })
}

pub fn emit_workflow_inbox_update(app: &tauri::AppHandle, blueprint: &Blueprint, run_root: &Path) {
    emit_workflow_inbox_update_with_name(app, &blueprint.name, run_root);
}

pub fn emit_workflow_inbox_update_with_name(
    app: &tauri::AppHandle,
    workflow_name: &str,
    run_root: &Path,
) {
    if let Some(update) = workflow_inbox_update_with_name(workflow_name, run_root) {
        let _ = app.emit(WORKFLOW_INBOX_UPDATED_EVENT, update);
    }
}

/// Scan `<runs_dir>/<id>/<run>/state.json` for runs still marked Running.
/// Returns `(blueprint_id, run_id)` pairs for diagnostics and tests.
pub fn scan_interrupted_runs(runs_dir: &Path) -> Vec<(String, String)> {
    let mut interrupted = Vec::new();
    let Ok(blueprints) = std::fs::read_dir(runs_dir) else {
        return interrupted;
    };

    for blueprint in blueprints.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(runs) = std::fs::read_dir(blueprint.path()) else {
            continue;
        };

        for run in runs.flatten().filter(|entry| entry.path().is_dir()) {
            if let Ok(Some(state)) = read_checkpoint(&run.path()) {
                if state.status == RunStatus::Running {
                    interrupted.push((state.blueprint_id, state.run_id));
                }
            }
        }
    }

    interrupted
}

pub fn fail_interrupted_runs(runs_dir: &Path) -> Vec<(String, String)> {
    let mut interrupted = Vec::new();
    let Ok(blueprints) = std::fs::read_dir(runs_dir) else {
        return interrupted;
    };

    for blueprint in blueprints.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(runs) = std::fs::read_dir(blueprint.path()) else {
            continue;
        };

        for run in runs.flatten().filter(|entry| entry.path().is_dir()) {
            let run_root = run.path();
            let Ok(Some(checkpoint)) = read_checkpoint(&run_root) else {
                continue;
            };
            if checkpoint.status != RunStatus::Running {
                continue;
            }
            let message = "workflow run interrupted by application restart".to_string();
            let Ok(events) = read_events(&run_root) else {
                continue;
            };
            let Some(mut state) = recover_interrupted_state(&checkpoint, &run_root, &events) else {
                // Do not rewrite a run whose event log cannot be validated or
                // whose immutable graph snapshot cannot be recovered.
                continue;
            };
            if matches!(state.status, RunStatus::Completed | RunStatus::Failed) {
                if write_checkpoint(&run_root, &state).is_ok() {
                    interrupted.push((state.blueprint_id, state.run_id));
                }
                continue;
            }
            let already_recorded = events.iter().any(|event| {
                matches!(
                    &event.kind,
                    EventKind::RunFailed { error } if error == &message
                )
            });
            if !already_recorded {
                let Ok(event_next_seq) = Engine::validate_event_sequence(&events) else {
                    continue;
                };
                let event = Event::new(
                    event_next_seq,
                    EventKind::RunFailed {
                        error: message.clone(),
                    },
                );
                if append_event(&run_root, &event).is_err() {
                    continue;
                }
                state.next_seq = event.seq + 1;
            } else {
                let Ok(event_next_seq) = Engine::validate_event_sequence(&events) else {
                    continue;
                };
                state.next_seq = event_next_seq;
            }
            state.status = RunStatus::Failed;
            state.failure = Some(message);
            if write_checkpoint(&run_root, &state).is_err() {
                continue;
            }
            interrupted.push((state.blueprint_id, state.run_id));
        }
    }

    interrupted
}

fn recover_interrupted_state(
    checkpoint: &wardian_core::engine::RunState,
    run_root: &Path,
    events: &[Event],
) -> Option<wardian_core::engine::RunState> {
    if let Ok(Some(snapshot)) = read_blueprint_snapshot(run_root) {
        return Engine::replay(&snapshot, run_root).ok();
    }
    if run_root.join("blueprint.json").exists() {
        return None;
    }
    if let Some(path) = wardian_core::workflow::resolve_blueprint_path(&checkpoint.blueprint_id) {
        if let Ok(blueprint) = wardian_core::workflow::parse_file(&path) {
            return Engine::replay(&blueprint, run_root).ok();
        }
        return None;
    }

    // Pre-snapshot runs may have no graph available. Preserve their checkpoint
    // state, but still validate the append-only sequence and advance to the
    // actual log tail before appending the restart failure.
    let event_next_seq = Engine::validate_event_sequence(events).ok()?;
    if checkpoint.next_seq > event_next_seq {
        return None;
    }
    let mut state = checkpoint.clone();
    state.next_seq = event_next_seq;
    Some(state)
}

/// Build the live executor for a run in `workspace` with `default_provider`.
pub fn live_executor(
    workspace: PathBuf,
    default_provider: String,
    bindings: HashMap<String, String>,
) -> LiveStepExecutor {
    live_executor_with_catalog(workspace, default_provider, bindings, HashMap::new())
}

pub fn live_executor_with_catalog(
    workspace: PathBuf,
    default_provider: String,
    bindings: HashMap<String, String>,
    agent_catalog: HashMap<String, AgentBinding>,
) -> LiveStepExecutor {
    LiveStepExecutor::new(
        Arc::new(HeadlessAgentRunner),
        workspace,
        default_provider,
        bindings,
        agent_catalog,
    )
}

pub fn live_executor_with_catalog_and_assignments(
    workspace: PathBuf,
    default_provider: String,
    bindings: HashMap<String, String>,
    assignments: WorkflowAssignments,
    agent_catalog: HashMap<String, AgentBinding>,
) -> LiveStepExecutor {
    LiveStepExecutor::new_with_assignments_and_live_runner(
        Arc::new(HeadlessAgentRunner),
        None,
        workspace,
        default_provider,
        bindings,
        assignments,
        agent_catalog,
    )
}

pub fn live_executor_with_catalog_and_app(
    app: tauri::AppHandle,
    workspace: PathBuf,
    default_provider: String,
    bindings: HashMap<String, String>,
    agent_catalog: HashMap<String, AgentBinding>,
) -> LiveStepExecutor {
    LiveStepExecutor::new_with_live_runner(
        Arc::new(TauriHeadlessAgentRunner::new(app.clone())),
        Some(Arc::new(TauriLiveAgentRunner::new(app.clone()))),
        workspace,
        default_provider,
        bindings,
        agent_catalog,
    )
    .with_notification_app(app)
}

pub fn live_executor_with_catalog_assignments_and_app(
    app: tauri::AppHandle,
    workspace: PathBuf,
    default_provider: String,
    bindings: HashMap<String, String>,
    assignments: WorkflowAssignments,
    agent_catalog: HashMap<String, AgentBinding>,
) -> LiveStepExecutor {
    LiveStepExecutor::new_with_assignments_and_live_runner(
        Arc::new(TauriHeadlessAgentRunner::new(app.clone())),
        Some(Arc::new(TauriLiveAgentRunner::new(app.clone()))),
        workspace,
        default_provider,
        bindings,
        assignments,
        agent_catalog,
    )
    .with_notification_app(app)
}

fn invocation_path(run_root: &Path) -> PathBuf {
    run_root.join("invocation.json")
}

pub fn write_run_invocation(
    run_root: &Path,
    provider: &str,
    workspace: &Path,
    bindings: &HashMap<String, String>,
    assignments: &WorkflowAssignments,
) -> Result<(), String> {
    write_run_invocation_with_schedule_id(
        run_root,
        provider,
        workspace,
        bindings,
        assignments,
        None,
    )
}

pub fn write_run_invocation_with_schedule_id(
    run_root: &Path,
    provider: &str,
    workspace: &Path,
    bindings: &HashMap<String, String>,
    assignments: &WorkflowAssignments,
    schedule_id: Option<String>,
) -> Result<(), String> {
    write_run_invocation_with_authority(
        run_root,
        provider,
        workspace,
        bindings,
        assignments,
        schedule_id,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn write_run_invocation_with_authority(
    run_root: &Path,
    provider: &str,
    workspace: &Path,
    bindings: &HashMap<String, String>,
    assignments: &WorkflowAssignments,
    schedule_id: Option<String>,
    memory_principal: Option<String>,
) -> Result<(), String> {
    std::fs::create_dir_all(run_root)
        .map_err(|error| format!("failed to create run directory: {error}"))?;
    let invocation = WorkflowRunInvocation {
        schema: 2,
        provider: provider.to_string(),
        workspace: workspace.to_string_lossy().to_string(),
        schedule_id,
        bindings: bindings.clone(),
        assignments: assignments.clone(),
        memory_principal,
    };
    let body = serde_json::to_string_pretty(&invocation)
        .map_err(|error| format!("failed to serialize workflow invocation: {error}"))?;
    std::fs::write(invocation_path(run_root), body)
        .map_err(|error| format!("failed to write workflow invocation: {error}"))
}

pub fn read_run_invocation(run_root: &Path) -> Result<Option<WorkflowRunInvocation>, String> {
    let path = invocation_path(run_root);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read workflow invocation: {error}"))?;
    serde_json::from_str(&content)
        .map(Some)
        .map_err(|error| format!("failed to parse workflow invocation: {error}"))
}

pub async fn agent_catalog_from_state(
    state: &AppState,
    bindings: &HashMap<String, String>,
    workspace: &Path,
    default_provider: &str,
) -> HashMap<String, AgentBinding> {
    agent_catalog_from_state_with_assignments(
        state,
        bindings,
        &WorkflowAssignments::new(),
        workspace,
        default_provider,
    )
    .await
}

pub async fn agent_catalog_from_state_with_assignments(
    state: &AppState,
    bindings: &HashMap<String, String>,
    assignments: &WorkflowAssignments,
    workspace: &Path,
    default_provider: &str,
) -> HashMap<String, AgentBinding> {
    let mut catalog = HashMap::new();
    {
        let agents = state.agents.lock().await;
        for agent in agents.values() {
            if let Ok(config) = agent.config.lock() {
                let current_status = agent
                    .current_status
                    .lock()
                    .map(|status| status.clone())
                    .unwrap_or_default();
                let normalized_status = wardian_core::identity::normalize_status(&current_status);
                let is_live = !config.is_off && agent.runtime_generation.is_some();
                let is_input_ready = is_live
                    && !matches!(
                        normalized_status.as_str(),
                        "processing" | "action_required" | "headless" | "off" | "error"
                    );
                if let Some(binding) = agent_binding_from_config(
                    &config,
                    workspace,
                    default_provider,
                    is_live,
                    is_input_ready,
                ) {
                    catalog.insert(binding.session_id.clone(), binding);
                }
            }
        }
    }

    for target in bindings.values() {
        if catalog.contains_key(target) {
            continue;
        }
        if let Some(config) = crate::manager::persisted_agent_config(target) {
            if let Some(binding) =
                agent_binding_from_config(&config, workspace, default_provider, false, false)
            {
                catalog.insert(binding.session_id.clone(), binding);
            }
        }
    }

    for assignment in assignments.values() {
        let WorkflowRoleAssignment::Agent { agent_id, .. } = assignment else {
            continue;
        };
        if catalog.contains_key(agent_id) {
            continue;
        }
        if let Some(config) = crate::manager::persisted_agent_config(agent_id) {
            if let Some(binding) =
                agent_binding_from_config(&config, workspace, default_provider, false, false)
            {
                catalog.insert(binding.session_id.clone(), binding);
            }
        }
    }

    catalog
}

fn agent_binding_from_config(
    config: &AgentConfig,
    workspace: &Path,
    default_provider: &str,
    is_live: bool,
    is_input_ready: bool,
) -> Option<AgentBinding> {
    let session_id = config.session_id.trim();
    if session_id.is_empty() {
        return None;
    }

    let provider = if config.provider.trim().is_empty() {
        default_provider.to_string()
    } else {
        config.provider.clone()
    };
    let cwd = if config.folder.trim().is_empty() {
        workspace.to_path_buf()
    } else {
        PathBuf::from(&config.folder)
    };

    Some(AgentBinding {
        session_id: session_id.to_string(),
        provider,
        cwd,
        resume_session: config.resume_session.clone(),
        is_live,
        is_input_ready,
        config: Some(config.clone()),
    })
}

/// Drive a fresh run to completion or pause.
pub async fn drive_new_run(
    blueprint: Blueprint,
    run_id: String,
    run_root: PathBuf,
    workspace: PathBuf,
    default_provider: String,
    input: Value,
    bindings: HashMap<String, String>,
) -> Result<(), String> {
    drive_new_run_with_catalog(
        None,
        blueprint,
        run_id,
        run_root,
        workspace,
        default_provider,
        input,
        bindings,
        HashMap::new(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn drive_new_run_with_catalog(
    app: Option<tauri::AppHandle>,
    blueprint: Blueprint,
    run_id: String,
    run_root: PathBuf,
    workspace: PathBuf,
    default_provider: String,
    input: Value,
    bindings: HashMap<String, String>,
    agent_catalog: HashMap<String, AgentBinding>,
) -> Result<(), String> {
    let assignments = wardian_core::workflow::assignment::normalize_assignments(
        None,
        &bindings,
        InvocationKind::Manual,
    );
    drive_new_run_with_catalog_and_assignments(
        app,
        blueprint,
        run_id,
        run_root,
        workspace,
        default_provider,
        input,
        bindings,
        assignments,
        agent_catalog,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn drive_new_run_with_catalog_and_assignments(
    app: Option<tauri::AppHandle>,
    blueprint: Blueprint,
    run_id: String,
    run_root: PathBuf,
    workspace: PathBuf,
    default_provider: String,
    input: Value,
    bindings: HashMap<String, String>,
    assignments: WorkflowAssignments,
    agent_catalog: HashMap<String, AgentBinding>,
) -> Result<(), String> {
    let state = prepare_new_run_with_assignments(
        &blueprint,
        &run_id,
        &run_root,
        &workspace,
        &default_provider,
        &bindings,
        &assignments,
        input,
    )?;
    drive_started_run_with_catalog_and_assignments(
        app,
        blueprint,
        state,
        run_root,
        workspace,
        default_provider,
        bindings,
        assignments,
        agent_catalog,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_new_run_with_assignments(
    blueprint: &Blueprint,
    run_id: &str,
    run_root: &Path,
    workspace: &Path,
    default_provider: &str,
    bindings: &HashMap<String, String>,
    assignments: &WorkflowAssignments,
    input: Value,
) -> Result<wardian_core::engine::RunState, String> {
    prepare_new_run_with_assignments_and_memory_principal(
        blueprint,
        run_id,
        run_root,
        workspace,
        default_provider,
        bindings,
        assignments,
        input,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_new_run_with_assignments_and_memory_principal(
    blueprint: &Blueprint,
    run_id: &str,
    run_root: &Path,
    workspace: &Path,
    default_provider: &str,
    bindings: &HashMap<String, String>,
    assignments: &WorkflowAssignments,
    input: Value,
    memory_principal: Option<String>,
) -> Result<wardian_core::engine::RunState, String> {
    write_run_invocation_with_authority(
        run_root,
        default_provider,
        workspace,
        bindings,
        assignments,
        None,
        memory_principal,
    )?;
    Engine::initialize_with_id(blueprint, run_id.to_string(), input, run_root)
        .map_err(|err| err.to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_new_scheduled_run_with_assignments(
    blueprint: &Blueprint,
    run_id: &str,
    run_root: &Path,
    workspace: &Path,
    default_provider: &str,
    bindings: &HashMap<String, String>,
    assignments: &WorkflowAssignments,
    schedule_id: &str,
    input: Value,
) -> Result<wardian_core::engine::RunState, String> {
    write_run_invocation_with_schedule_id(
        run_root,
        default_provider,
        workspace,
        bindings,
        assignments,
        Some(schedule_id.to_string()),
    )?;
    Engine::initialize_with_id(blueprint, run_id.to_string(), input, run_root)
        .map_err(|err| err.to_string())
}

#[allow(clippy::too_many_arguments)]
pub async fn drive_started_run_with_catalog_and_assignments(
    app: Option<tauri::AppHandle>,
    blueprint: Blueprint,
    state: wardian_core::engine::RunState,
    run_root: PathBuf,
    workspace: PathBuf,
    default_provider: String,
    bindings: HashMap<String, String>,
    assignments: WorkflowAssignments,
    agent_catalog: HashMap<String, AgentBinding>,
) -> Result<(), String> {
    drive_started_run_with_catalog_assignments_and_memory_principal(
        app,
        blueprint,
        state,
        run_root,
        workspace,
        default_provider,
        bindings,
        assignments,
        agent_catalog,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn drive_started_run_with_catalog_assignments_and_memory_principal(
    app: Option<tauri::AppHandle>,
    blueprint: Blueprint,
    state: wardian_core::engine::RunState,
    run_root: PathBuf,
    workspace: PathBuf,
    default_provider: String,
    bindings: HashMap<String, String>,
    assignments: WorkflowAssignments,
    agent_catalog: HashMap<String, AgentBinding>,
    memory_principal: Option<String>,
) -> Result<(), String> {
    let _headless_execution =
        wardian_core::workflow_execution_lock::acquire_headless_execution_guard()?;
    let owner_id = format!("{}/{}", blueprint.id, state.run_id);
    let exec = if let Some(app) = app {
        live_executor_with_catalog_assignments_and_app(
            app,
            workspace,
            default_provider,
            bindings,
            assignments,
            agent_catalog,
        )
    } else {
        live_executor_with_catalog_and_assignments(
            workspace,
            default_provider,
            bindings,
            assignments,
            agent_catalog,
        )
    }
    .with_owner_id(owner_id);
    let exec = match memory_principal {
        Some(agent_id) => exec.with_memory_principal(agent_id),
        None => exec,
    };
    Engine::drive_from_state(&blueprint, state, &run_root, &exec)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// Resume an interrupted or paused run.
pub async fn drive_resume(
    blueprint: Blueprint,
    run_root: PathBuf,
    workspace: PathBuf,
    default_provider: String,
    bindings: HashMap<String, String>,
) -> Result<(), String> {
    drive_resume_with_catalog(
        None,
        blueprint,
        run_root,
        workspace,
        default_provider,
        bindings,
        HashMap::new(),
    )
    .await
}

pub async fn drive_resume_with_catalog(
    app: Option<tauri::AppHandle>,
    blueprint: Blueprint,
    run_root: PathBuf,
    workspace: PathBuf,
    default_provider: String,
    bindings: HashMap<String, String>,
    agent_catalog: HashMap<String, AgentBinding>,
) -> Result<(), String> {
    let _headless_execution =
        wardian_core::workflow_execution_lock::acquire_headless_execution_guard()?;
    let assignments = wardian_core::workflow::assignment::normalize_assignments(
        None,
        &bindings,
        InvocationKind::Manual,
    );
    let owner_id = run_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(|run_id| format!("{}/{}", blueprint.id, run_id))
        .unwrap_or_else(|| format!("{}/resume", blueprint.id));
    let exec = if let Some(app) = app {
        live_executor_with_catalog_assignments_and_app(
            app,
            workspace,
            default_provider,
            bindings,
            assignments,
            agent_catalog,
        )
    } else {
        live_executor_with_catalog_and_assignments(
            workspace,
            default_provider,
            bindings,
            assignments,
            agent_catalog,
        )
    }
    .with_owner_id(owner_id);
    Engine::resume(&blueprint, &run_root, &exec)
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;
    use wardian_core::engine::{
        event::EventKind,
        store::{read_checkpoint, read_events},
        RunState, RunStatus,
    };
    use wardian_core::models::{AgentConversationMode, BusyPolicy, WorkflowRoleAssignment};

    const INVOKER_BLUEPRINT: &str = r#"---
schema: 2
id: invoker
name: Invoker
nodes:
  - id: trigger-1
    type: manual_trigger
    fields:
      input_schema: '{"type":"object","properties":{"symbol":{"type":"string"}}}'
  - id: analyze
    type: task
    fields:
      agent: role:analyst
      prompt: Analyze {{trigger.output.symbol}}
edges:
  - from: trigger-1
    to: analyze
---

# Invoker
"#;

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        previous_home: Option<std::ffi::OsString>,
        previous_session_id: Option<std::ffi::OsString>,
        previous_mock_scenario: Option<std::ffi::OsString>,
        previous_mock_delay: Option<std::ffi::OsString>,
        previous_mock_script: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(home: &std::path::Path, mock_script: &std::path::Path) -> Self {
            let guard = Self {
                _lock: crate::utils::wardian_test_env_lock(),
                previous_home: std::env::var_os("WARDIAN_HOME"),
                previous_session_id: std::env::var_os("WARDIAN_SESSION_ID"),
                previous_mock_scenario: std::env::var_os("WARDIAN_MOCK_SCENARIO"),
                previous_mock_delay: std::env::var_os("WARDIAN_MOCK_DELAY_MS"),
                previous_mock_script: std::env::var_os("WARDIAN_MOCK_SCRIPT"),
            };

            std::env::set_var("WARDIAN_HOME", home);
            std::env::remove_var("WARDIAN_SESSION_ID");
            std::env::set_var("WARDIAN_MOCK_SCENARIO", "basic");
            std::env::set_var("WARDIAN_MOCK_DELAY_MS", "0");
            std::env::set_var("WARDIAN_MOCK_SCRIPT", mock_script);

            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            restore_env("WARDIAN_HOME", self.previous_home.take());
            restore_env("WARDIAN_SESSION_ID", self.previous_session_id.take());
            restore_env("WARDIAN_MOCK_SCENARIO", self.previous_mock_scenario.take());
            restore_env("WARDIAN_MOCK_DELAY_MS", self.previous_mock_delay.take());
            restore_env("WARDIAN_MOCK_SCRIPT", self.previous_mock_script.take());
        }
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn mock_script_path() -> std::path::PathBuf {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("scripts")
            .join("mock-agent.cjs");
        assert!(script.exists(), "mock-agent.cjs not found at {:?}", script);
        script
    }

    const EXECUTOR_BLUEPRINT: &str = r#"---
schema: 2
id: executor
name: Executor
nodes:
  - id: trigger-1
    type: manual_trigger
  - id: plan
    type: task
    fields:
      agent: role:coder
      prompt: Return a tiny plan
edges:
  - from: trigger-1
    to: plan
---

# Executor
"#;

    #[test]
    fn scan_interrupted_marks_running_runs() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path().join("wf").join("run-1");
        std::fs::create_dir_all(&run_root).unwrap();
        let mut state = RunState::new("run-1", "wf");
        state.status = RunStatus::Running;
        wardian_core::engine::store::write_checkpoint(&run_root, &state).unwrap();

        let interrupted = scan_interrupted_runs(dir.path());
        assert_eq!(interrupted, vec![("wf".to_string(), "run-1".to_string())]);
    }

    #[test]
    fn workflow_inbox_update_projects_approval_and_terminal_state_from_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path().join("wf").join("run-1");
        let blueprint = wardian_core::workflow::parse::parse_str(INVOKER_BLUEPRINT).unwrap();
        let mut state = RunState::new("run-1", "invoker");
        state.status = RunStatus::AwaitingApproval;
        write_checkpoint(&run_root, &state).unwrap();

        let approval = workflow_inbox_update(&blueprint, &run_root).unwrap();
        assert_eq!(approval.status, "awaiting_approval");
        assert_eq!(approval.workflow_name, "Invoker");
        assert_eq!(approval.summary, None);

        append_event(
            &run_root,
            &Event::new(
                state.next_seq,
                EventKind::NodeCompleted {
                    node: "analyze".to_string(),
                    output: serde_json::json!({ "text": "Workflow result" }),
                },
            ),
        )
        .unwrap();
        state.status = RunStatus::Completed;
        write_checkpoint(&run_root, &state).unwrap();

        let completed = workflow_inbox_update(&blueprint, &run_root).unwrap();
        assert_eq!(completed.status, "completed");
        assert_eq!(completed.summary.as_deref(), Some("Workflow result"));
    }

    #[test]
    fn workflow_inbox_update_includes_terminal_failure_reason() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path().join("wf").join("run-1");
        let blueprint = wardian_core::workflow::parse::parse_str(INVOKER_BLUEPRINT).unwrap();
        let mut state = RunState::new("run-1", "invoker");
        state.status = RunStatus::Failed;
        state.failure = Some("approval rejected".to_string());
        write_checkpoint(&run_root, &state).unwrap();

        let update = workflow_inbox_update(&blueprint, &run_root).unwrap();
        assert_eq!(update.status, "failed");
        assert_eq!(update.error.as_deref(), Some("approval rejected"));
    }

    #[test]
    fn fail_interrupted_runs_marks_running_checkpoint_failed() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path().join("wf").join("run-1");
        std::fs::create_dir_all(&run_root).unwrap();
        let mut state = RunState::new("run-1", "wf");
        state.status = RunStatus::Running;
        wardian_core::engine::store::write_checkpoint(&run_root, &state).unwrap();

        let interrupted = fail_interrupted_runs(dir.path());

        assert_eq!(interrupted, vec![("wf".to_string(), "run-1".to_string())]);
        let state = wardian_core::engine::store::read_checkpoint(&run_root)
            .unwrap()
            .unwrap();
        assert_eq!(state.status, RunStatus::Failed);
        assert_eq!(
            state.failure.as_deref(),
            Some("workflow run interrupted by application restart")
        );
        let events = read_events(&run_root).unwrap();
        assert!(matches!(
            events.last().map(|event| &event.kind),
            Some(EventKind::RunFailed { .. })
        ));
    }

    #[test]
    fn fail_interrupted_runs_does_not_duplicate_existing_restart_failure() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path().join("wf").join("run-1");
        std::fs::create_dir_all(&run_root).unwrap();
        let mut state = RunState::new("run-1", "wf");
        state.status = RunStatus::Running;
        wardian_core::engine::store::write_checkpoint(&run_root, &state).unwrap();
        wardian_core::engine::store::append_event(
            &run_root,
            &Event::new(
                0,
                EventKind::RunFailed {
                    error: "workflow run interrupted by application restart".to_string(),
                },
            ),
        )
        .unwrap();

        let interrupted = fail_interrupted_runs(dir.path());

        assert_eq!(interrupted, vec![("wf".to_string(), "run-1".to_string())]);
        let events = read_events(&run_root).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.kind, EventKind::RunFailed { .. }))
                .count(),
            1
        );
        let state = wardian_core::engine::store::read_checkpoint(&run_root)
            .unwrap()
            .unwrap();
        assert_eq!(state.next_seq, 1);
        assert_eq!(state.status, RunStatus::Failed);
    }

    #[test]
    fn fail_interrupted_runs_folds_event_tail_before_appending_restart_failure() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path().join("wf").join("run-1");
        let blueprint = wardian_core::workflow::parse::parse_str(
            r#"---
schema: 2
id: wf
name: Workflow
nodes:
  - id: trigger
    type: manual_trigger
edges: []
---

# Workflow
"#,
        )
        .unwrap();
        Engine::initialize_with_id(&blueprint, "run-1", serde_json::json!({}), &run_root).unwrap();
        append_event(
            &run_root,
            &Event::new(
                1,
                EventKind::NodeCompleted {
                    node: "trigger".into(),
                    output: serde_json::json!({"recovered": true}),
                },
            ),
        )
        .unwrap();

        let interrupted = fail_interrupted_runs(dir.path());

        assert_eq!(interrupted, vec![("wf".to_string(), "run-1".to_string())]);
        let state = wardian_core::engine::store::read_checkpoint(&run_root)
            .unwrap()
            .unwrap();
        assert_eq!(state.status, RunStatus::Failed);
        assert_eq!(state.next_seq, 3);
        assert_eq!(
            state.registry["nodes"]["trigger"]["output"]["recovered"],
            true
        );
        let events = read_events(&run_root).unwrap();
        assert_eq!(
            events.iter().map(|event| event.seq).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn run_invocation_round_trips_assignments_and_memory_principal() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path().join("wf").join("run-1");
        let mut bindings = HashMap::new();
        bindings.insert("planner".to_string(), "agent-1".to_string());
        let mut assignments = WorkflowAssignments::new();
        assignments.insert(
            "planner".to_string(),
            WorkflowRoleAssignment::Agent {
                agent_id: "agent-1".to_string(),
                conversation: AgentConversationMode::Current,
                busy_policy: BusyPolicy::Skip,
            },
        );

        write_run_invocation_with_authority(
            &run_root,
            "mock",
            std::path::Path::new("/workspace"),
            &bindings,
            &assignments,
            None,
            Some("agent-1".to_string()),
        )
        .unwrap();

        let invocation = read_run_invocation(&run_root).unwrap().unwrap();
        assert_eq!(invocation.provider, "mock");
        assert_eq!(invocation.bindings, bindings);
        assert_eq!(invocation.assignments, assignments);
        assert_eq!(invocation.memory_principal.as_deref(), Some("agent-1"));
    }

    #[test]
    fn prepare_new_run_writes_invocation_and_started_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let run_root = dir.path().join("wf").join("run-1");
        let blueprint = wardian_core::workflow::parse::parse_str(INVOKER_BLUEPRINT).unwrap();
        let bindings = HashMap::from([("analyst".to_string(), "mock".to_string())]);
        let assignments = wardian_core::workflow::assignment::normalize_assignments(
            None,
            &bindings,
            InvocationKind::Manual,
        );

        let state = prepare_new_run_with_assignments(
            &blueprint,
            "run-1",
            &run_root,
            dir.path(),
            "mock",
            &bindings,
            &assignments,
            serde_json::json!({"symbol":"SPY"}),
        )
        .unwrap();

        assert_eq!(state.run_id, "run-1");
        assert!(run_root.join("invocation.json").is_file());
        assert!(run_root.join("events.jsonl").is_file());
        assert!(run_root.join("state.json").is_file());
        let events = read_events(&run_root).unwrap();
        assert!(matches!(
            events.first().map(|event| &event.kind),
            Some(EventKind::RunStarted { .. })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mock_provider_drives_a_workflow_run_to_completion() {
        let home = tempfile::tempdir().unwrap();
        let workflows_dir = home.path().join("library").join("workflows");
        std::fs::create_dir_all(&workflows_dir).unwrap();
        let blueprint_path = workflows_dir.join("executor.md");
        std::fs::write(&blueprint_path, EXECUTOR_BLUEPRINT).unwrap();

        let _env = EnvGuard::set(home.path(), &mock_script_path());
        let blueprint = wardian_core::workflow::parse_file(&blueprint_path).unwrap();
        let report = wardian_core::workflow::validate(&blueprint);
        assert!(report.is_valid(), "diagnostics: {:?}", report.diagnostics);

        let run_id = wardian_core::engine::driver::new_run_id();
        let run_root = wardian_core::paths::workflow_run_dir(&blueprint.id, &run_id).unwrap();
        drive_new_run(
            blueprint,
            run_id,
            run_root.clone(),
            home.path().to_path_buf(),
            "mock".into(),
            serde_json::json!({}),
            HashMap::new(),
        )
        .await
        .unwrap();

        let state = read_checkpoint(&run_root).unwrap().unwrap();
        assert_eq!(state.status, RunStatus::Completed);
        assert!(run_root.join("events.jsonl").is_file());
        assert!(state.node_output("plan").is_some());
        assert!(read_events(&run_root)
            .unwrap()
            .iter()
            .any(|event| matches!(event.kind, EventKind::NodeCompleted { ref node, .. } if node == "plan")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn input_interpolates_and_role_binding_selects_provider() {
        let home = tempfile::tempdir().unwrap();
        let workflows_dir = home.path().join("library").join("workflows");
        std::fs::create_dir_all(&workflows_dir).unwrap();
        let blueprint_path = workflows_dir.join("invoker.md");
        std::fs::write(&blueprint_path, INVOKER_BLUEPRINT).unwrap();

        let _env = EnvGuard::set(home.path(), &mock_script_path());

        let blueprint = wardian_core::workflow::parse_file(&blueprint_path).unwrap();
        let report = wardian_core::workflow::validate(&blueprint);
        assert!(report.is_valid(), "diagnostics: {:?}", report.diagnostics);

        let run_id = wardian_core::engine::driver::new_run_id();
        let run_root = wardian_core::paths::workflow_run_dir(&blueprint.id, &run_id).unwrap();
        drive_new_run(
            blueprint,
            run_id,
            run_root.clone(),
            home.path().to_path_buf(),
            "codex".into(),
            serde_json::json!({ "symbol": "SPY" }),
            HashMap::from([("analyst".to_string(), "mock".to_string())]),
        )
        .await
        .unwrap();

        let state = read_checkpoint(&run_root).unwrap().unwrap();
        assert_eq!(state.status, RunStatus::Completed);
        assert!(state.node_output("analyze").is_some());
        assert_eq!(state.registry["trigger"]["output"]["symbol"], "SPY");

        let events = read_events(&run_root).unwrap();
        let started = events.iter().find_map(|event| match &event.kind {
            EventKind::RunStarted { trigger, .. } => Some(trigger),
            _ => None,
        });
        assert_eq!(started, Some(&serde_json::json!({ "symbol": "SPY" })));
    }
}

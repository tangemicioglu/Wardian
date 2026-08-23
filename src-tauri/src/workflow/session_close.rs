//! Generic conversation-boundary workflow invoker.

use crate::workflow::runs;
use serde_json::Value;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use wardian_core::models::InvocationKind;

#[derive(Debug, Clone)]
pub struct SessionCloseContext {
    pub agent_id: String,
    pub agent_name: String,
    pub workspace: String,
    pub provider: String,
    pub boundary_reason: String,
    pub archive_available: bool,
    pub conversation_id: Option<String>,
    pub source_sequence: Option<u64>,
}

pub fn invoke_matching(app: AppHandle, context: SessionCloseContext) {
    let invokers =
        wardian_core::session_close::matching_invokers(&context.agent_id, &context.boundary_reason);
    for invoker in invokers {
        if !wardian_core::session_close::archive_requirement_satisfied(
            &invoker,
            context.archive_available,
        ) {
            crate::utils::logging::log_debug(&format!(
                "[workflow] session-close invoker {} skipped because no archive is available",
                invoker.id
            ));
            continue;
        }
        let app = app.clone();
        let context = context.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = launch(app, invoker, context).await {
                crate::utils::logging::log_debug(&format!(
                    "[workflow] session-close invocation failed: {error}"
                ));
            }
        });
    }
}

async fn launch(
    app: AppHandle,
    invoker: wardian_core::session_close::WorkflowSessionCloseInvoker,
    context: SessionCloseContext,
) -> Result<(), String> {
    let path = wardian_core::workflow::resolve_blueprint_path(&invoker.blueprint_id)
        .ok_or_else(|| format!("could not resolve blueprint {}", invoker.blueprint_id))?;
    let blueprint = wardian_core::workflow::parse_file(&path).map_err(|error| error.to_string())?;
    let report = wardian_core::workflow::validate(&blueprint);
    if !report.is_valid() {
        return Err(format!("blueprint {} is invalid", blueprint.id));
    }
    let run_id = wardian_core::engine::driver::new_run_id();
    let run_root = wardian_core::paths::workflow_run_dir(&blueprint.id, &run_id)
        .ok_or_else(|| "could not resolve workflow run directory".to_string())?;
    let provider = invoker.provider.unwrap_or_else(|| {
        crate::utils::load_shell_settings()
            .map(|settings| settings.default_provider)
            .unwrap_or_else(|_| "codex".to_string())
    });
    let workspace = invoker
        .workspace
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&context.workspace));
    let mut input = match invoker.input {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    input.insert("agent_id".into(), Value::String(context.agent_id));
    input.insert("agent_name".into(), Value::String(context.agent_name));
    input.insert("workspace".into(), Value::String(context.workspace));
    input.insert("source_provider".into(), Value::String(context.provider));
    input.insert(
        "boundary_reason".into(),
        Value::String(context.boundary_reason),
    );
    input.insert(
        "archive_available".into(),
        Value::Bool(context.archive_available),
    );
    input.insert(
        "conversation_id".into(),
        context
            .conversation_id
            .clone()
            .map(Value::String)
            .unwrap_or_else(|| Value::String(String::new())),
    );
    input.insert(
        "source_sequence".into(),
        Value::from(context.source_sequence.unwrap_or(0)),
    );
    input.insert(
        "idempotency_key".into(),
        Value::String(format!(
            "session-close:{}:{}:{}",
            invoker.id,
            context.conversation_id.as_deref().unwrap_or("no-archive"),
            context.source_sequence.unwrap_or(0)
        )),
    );
    let assignments = wardian_core::workflow::assignment::normalize_assignments(
        Some(invoker.assignments),
        &invoker.bindings,
        InvocationKind::Scheduled,
    );
    let state = app.state::<crate::state::AppState>();
    let catalog = runs::agent_catalog_from_state_with_assignments(
        &state,
        &invoker.bindings,
        &assignments,
        &workspace,
        &provider,
    )
    .await;
    let run_state = runs::prepare_new_run_with_assignments(
        &blueprint,
        &run_id,
        &run_root,
        &workspace,
        &provider,
        &invoker.bindings,
        &assignments,
        Value::Object(input),
    )?;
    let blueprint_for_inbox = blueprint.clone();
    let run_root_for_inbox = run_root.clone();
    let app_for_inbox = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = runs::drive_started_run_with_catalog_and_assignments(
            Some(app),
            blueprint,
            run_state,
            run_root,
            workspace,
            provider,
            invoker.bindings,
            assignments,
            catalog,
        )
        .await;
        if let Err(error) = result {
            crate::utils::logging::log_debug(&format!(
                "[workflow] session-close run failed: {error}"
            ));
        }
        runs::emit_workflow_inbox_update(&app_for_inbox, &blueprint_for_inbox, &run_root_for_inbox);
    });
    Ok(())
}

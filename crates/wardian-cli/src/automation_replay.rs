use crate::errors::CliError;
use crate::{automation_run_root, find_library_blueprint, render_json};

pub(crate) fn render(blueprint_id: &str, run_id: &str) -> Result<String, CliError> {
    let run_root = automation_run_root(blueprint_id, run_id)?;
    let events = wardian_core::engine::store::read_events(&run_root)
        .map_err(|error| CliError::generic(error.to_string()))?;
    let state = match events.first().map(|event| &event.kind) {
        Some(wardian_core::engine::EventKind::RunFailed { .. }) if events.len() == 1 => {
            wardian_core::engine::Engine::replay_launch_failure(blueprint_id, &run_root)
                .map_err(|error| CliError::generic(error.to_string()))?
        }
        _ => replay_graph(blueprint_id, &run_root)?,
    };
    render_json(serde_json::json!({
        "schema": 1,
        "state": state,
    }))
}

fn replay_graph(
    blueprint_id: &str,
    run_root: &std::path::Path,
) -> Result<wardian_core::engine::RunState, CliError> {
    let blueprint = wardian_core::engine::store::read_blueprint_snapshot(run_root)
        .map_err(|error| CliError::generic(error.to_string()))?
        .or(find_library_blueprint(blueprint_id)?)
        .ok_or_else(|| CliError::generic(format!("blueprint {blueprint_id} not found")))?;
    wardian_core::engine::Engine::replay(&blueprint, run_root)
        .map_err(|error| CliError::generic(error.to_string()))
}

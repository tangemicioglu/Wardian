//! Validation and parsing shared by every automation invoker CLI surface.
//!
//! Schedules, session-close invokers, and listeners all accept the same
//! blueprint, provider, input, and binding arguments. Keeping the checks here
//! rather than in `main` means a new invoker family reuses them by importing
//! rather than by threading callbacks through the command dispatcher.

use crate::errors::CliError;
use crate::json_input;
use std::collections::HashMap;

pub(crate) fn parse_automation_exec_input(
    input: Option<&str>,
) -> Result<serde_json::Value, CliError> {
    let Some(raw) = input else {
        return Ok(serde_json::json!({}));
    };
    json_input::parse(raw, "--input")
}

pub(crate) fn parse_automation_bindings(
    bind: &[String],
) -> Result<HashMap<String, String>, CliError> {
    let mut bindings = HashMap::new();
    for entry in bind {
        let Some((name, provider)) = entry.split_once('=') else {
            return Err(CliError::generic(format!(
                "invalid --bind `{entry}`; expected name=provider"
            )));
        };
        let name = name.trim();
        let provider = provider.trim();
        if name.is_empty() || provider.is_empty() {
            return Err(CliError::generic(format!(
                "invalid --bind `{entry}`; expected non-empty name=provider"
            )));
        }
        bindings.insert(name.to_string(), provider.to_string());
    }
    Ok(bindings)
}

pub(crate) fn validate_schedule_provider(provider: Option<&str>) -> Result<(), CliError> {
    if let Some(provider) = provider {
        if !wardian_core::automation::assignment::is_known_provider(provider) {
            return Err(CliError::generic(format!(
                "unsupported provider `{provider}`"
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_schedule_blueprint(
    blueprint_id: &str,
) -> Result<wardian_core::automation::Blueprint, CliError> {
    let path = wardian_core::automation::resolve_blueprint_path(blueprint_id).ok_or_else(|| {
        CliError::generic(format!(
            "blueprint not found in library/automations: {blueprint_id}"
        ))
    })?;
    let blueprint = wardian_core::automation::parse_file(&path).map_err(|error| {
        CliError::generic(format!("could not parse blueprint {blueprint_id}: {error}"))
    })?;
    let report = wardian_core::automation::validate(&blueprint);
    if !report.is_valid() {
        let diagnostics = serde_json::to_string(&report.diagnostics)
            .map_err(|error| CliError::generic(error.to_string()))?;
        return Err(CliError::generic(format!(
            "blueprint {blueprint_id} is invalid: {diagnostics}"
        )));
    }
    Ok(blueprint)
}

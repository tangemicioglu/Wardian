//! Shallow command discovery derived from the same Clap tree used for parsing.
//! This describes syntax, not response DTOs or all runtime validation rules.

use clap::{CommandFactory, ValueHint};
use serde_json::{json, Value};

use crate::args::{BrowserTargetArgs, Cli};
use crate::errors::{CliError, ExitCode};

/// Describe one command and its immediate children without opening Wardian state.
pub fn render(path: &[String]) -> Result<String, CliError> {
    let mut command = Cli::command();
    command.build();
    let mut names = vec!["wardian".to_string()];
    for part in path {
        // Browser uses a second parser after the user-supplied session target.
        if names == ["wardian", "browser"] && part == "<target>" {
            command = BrowserTargetArgs::command();
            command.build();
        } else {
            command = command.find_subcommand(part).cloned().ok_or_else(|| {
                let mut error = CliError::backend(
                    ExitCode::NotFound,
                    "unknown_command",
                    format!("unknown command `{part}` under `{}`", names.join(" ")),
                );
                error.hint = Some(format!(
                    "Run `wardian schema {}` for available commands.",
                    names[1..].join(" ")
                ));
                error
            })?;
        }
        names.push(part.clone());
    }
    command = command.bin_name(names.join(" "));
    command.build();
    let mut body = json!({
        "schema": 1,
        "command": names.join(" "),
        "description": command.get_about().map(ToString::to_string).unwrap_or_default(),
        "usage": command.render_usage().to_string(),
    });
    let args: Vec<Value> = command
        .get_arguments()
        .filter(|arg| !arg.is_hide_set() && !["help", "version"].contains(&arg.get_id().as_str()))
        .map(|arg| {
            let name = arg
                .get_long()
                .map(|name| format!("--{name}"))
                .unwrap_or_else(|| arg.get_id().to_string());
            let mut value = json!({"name": name});
            if arg.get_action().takes_values() {
                value["value"] = json!(arg
                    .get_value_names()
                    .map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>())
                    .unwrap_or_else(|| vec![arg.get_id().to_string().to_uppercase()]));
            }
            if arg.is_required_set() {
                value["required"] = json!(true);
            }
            if matches!(
                arg.get_action(),
                clap::ArgAction::Append | clap::ArgAction::Count
            ) || arg
                .get_num_args()
                .is_some_and(|range| range.max_values() > 1)
            {
                value["multiple"] = json!(true);
            }
            if let Some(help) = arg.get_help() {
                if !help.to_string().is_empty() {
                    value["description"] = json!(help.to_string());
                }
            }
            let defaults = arg.get_default_values();
            if arg.get_action().takes_values() && !defaults.is_empty() {
                value["default"] = json!(defaults
                    .iter()
                    .map(|v| v.to_string_lossy())
                    .collect::<Vec<_>>());
            }
            let choices: Vec<_> = arg
                .get_possible_values()
                .into_iter()
                .filter(|v| !v.is_hide_set())
                .map(|v| v.get_name().to_string())
                .collect();
            if !choices.is_empty() {
                value["choices"] = json!(choices);
            }
            if arg.get_value_hint() != ValueHint::Unknown {
                value["value_hint"] = json!(format!("{:?}", arg.get_value_hint()));
            }
            let conflicts = command.get_arg_conflicts_with(arg);
            if !conflicts.is_empty() {
                value["conflicts"] = json!(conflicts
                    .iter()
                    .map(|arg| arg
                        .get_long()
                        .map(|name| format!("--{name}"))
                        .unwrap_or_else(|| arg.get_id().to_string()))
                    .collect::<Vec<_>>());
            }
            value
        })
        .collect();
    if !args.is_empty() {
        body["args"] = json!(args);
    }
    let mut children: Vec<Value> = command.get_subcommands()
        .filter(|child| !child.is_hide_set() && child.get_name() != "help")
        .map(|child| json!({"name": child.get_name(), "description": child.get_about().map(ToString::to_string).unwrap_or_default()}))
        .collect();
    if names == ["wardian", "browser"] {
        children.push(json!({"name": "<target>", "description": "Actions on a browser session; use schema browser '<target>' to discover them."}));
    }
    if !children.is_empty() {
        body["commands"] = json!(children);
    }
    serde_json::to_string(&body)
        .map(|text| text + "\n")
        .map_err(|error| CliError::generic(error.to_string()))
}

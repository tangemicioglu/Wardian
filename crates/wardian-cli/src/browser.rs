//! `wardian browser` — the one interface humans and agents both use.
//!
//! Sessions are addressed as `browser:N` (short ref) or by id, matching how
//! the workbench labels them. Every subcommand supports `--json` so an agent
//! can branch on structure instead of parsing prose.

use clap::Parser;

use crate::args::{BrowserArgs, BrowserCommand, BrowserTargetArgs, BrowserTargetCommand};
use crate::errors::{CliError, ExitCode};
use crate::live;
use wardian_core::browser::{
    render_session_line, render_snapshot, BrowserActionResult, BrowserSessionSummary, ConsoleEntry,
    PageSnapshot,
};

/// Serializes a response under the CLI's standard envelope.
fn json_envelope<T: serde::Serialize>(key: &str, value: &T) -> Result<String, CliError> {
    let envelope = serde_json::json!({ "schema": 1, key: value });
    serde_json::to_string_pretty(&envelope)
        .map(|text| format!("{text}\n"))
        .map_err(|error| CliError::generic(error.to_string()))
}

/// Splits `wardian browser <target> <verb> ...` into its two halves.
///
/// Returns an error naming the missing half rather than letting clap report a
/// confusing "unrecognized subcommand" for what is really a missing target.
pub fn split_target_invocation(tokens: &[String]) -> Result<(String, Vec<String>), CliError> {
    let mut iter = tokens.iter();
    let target = iter
        .next()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            CliError::generic("browser needs a session, e.g. `wardian browser browser:1 reload`")
        })?;
    let rest: Vec<String> = iter.cloned().collect();
    if rest.is_empty() {
        return Err(CliError::generic(format!(
            "`wardian browser {target}` needs an action, e.g. `snapshot`, `click e2`, or `get url`"
        )));
    }
    Ok((target, rest))
}

/// Renders a summary for humans, or as JSON when asked.
fn emit_summary(summary: &BrowserSessionSummary, json: bool) -> Result<String, CliError> {
    if json {
        return json_envelope("session", summary);
    }
    Ok(format!("{}\n", render_session_line(summary)))
}

fn emit_snapshot(snapshot: &PageSnapshot, json: bool) -> Result<String, CliError> {
    if json {
        return json_envelope("snapshot", snapshot);
    }
    Ok(format!("{}\n", render_snapshot(snapshot)))
}

/// Renders an action result, including the folded re-snapshot when present.
fn emit_action(result: &BrowserActionResult, json: bool) -> Result<String, CliError> {
    if json {
        return json_envelope("action", result);
    }
    let mut text = format!("{} {}\n", result.action, result.element_ref);
    if let Some(snapshot) = result.snapshot.as_ref() {
        text.push_str(&render_snapshot(snapshot));
        text.push('\n');
    }
    Ok(text)
}

fn emit_console(entries: &[ConsoleEntry], json: bool) -> Result<String, CliError> {
    if json {
        return json_envelope("console", &entries);
    }
    if entries.is_empty() {
        return Ok("no console messages since the last navigation\n".to_string());
    }
    Ok(entries
        .iter()
        .map(|entry| format!("{}  {}", entry.level, entry.text))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

pub fn handle_browser(args: BrowserArgs) -> Result<String, CliError> {
    let json = args.json;
    match args.command {
        BrowserCommand::Open {
            url,
            agent,
            workspace,
            width,
            height,
            detached,
        } => {
            // An agent that does not name an owner owns what it opens, so its
            // sessions are closed with it rather than outliving it.
            let owner = agent.or_else(live::current_session_id);
            let summary = live::browser_open(url, owner, workspace, width, height, detached)
                .map_err(crate::control_error)?;
            emit_summary(&summary, json)
        }
        BrowserCommand::List => {
            let sessions = live::browser_list().map_err(crate::control_error)?;
            if json {
                return json_envelope("sessions", &sessions);
            }
            if sessions.is_empty() {
                return Ok("no browser sessions are open\n".to_string());
            }
            Ok(sessions
                .iter()
                .map(render_session_line)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n")
        }
        BrowserCommand::Target(tokens) => {
            let (target, rest) = split_target_invocation(&tokens)?;
            let parsed = BrowserTargetArgs::try_parse_from(rest)
                .map_err(|error| CliError::generic(error.to_string()))?;
            handle_target(&target, parsed.command, json || parsed.json)
        }
    }
}

/// Runs one verb against an already-resolved session.
fn handle_target(
    target: &str,
    command: BrowserTargetCommand,
    json: bool,
) -> Result<String, CliError> {
    let act = |element_ref: String, action: &str, value: Option<String>, snapshot_after: bool| {
        live::browser_act(target, &element_ref, action, value, snapshot_after)
            .map_err(crate::control_error)
            .and_then(|result| emit_action(&result, json))
    };

    match command {
        BrowserTargetCommand::Close => {
            live::browser_close(target).map_err(crate::control_error)?;
            Ok(if json {
                json_envelope("closed", &target)?
            } else {
                format!("closed {target}\n")
            })
        }
        BrowserTargetCommand::Navigate { action } => {
            let summary = live::browser_navigate(target, &action).map_err(crate::control_error)?;
            emit_summary(&summary, json)
        }
        BrowserTargetCommand::Get { field, selector } => {
            let result = live::browser_get(target, &field, selector).map_err(crate::control_error)?;
            if json {
                return json_envelope("result", &result);
            }
            Ok(format!("{}\n", result.value))
        }
        BrowserTargetCommand::Wait {
            load_state,
            selector,
            text,
            url_contains,
            function,
            timeout_ms,
        } => {
            let summary = live::browser_wait(
                target,
                load_state,
                selector,
                text,
                url_contains,
                function,
                timeout_ms,
            )
            .map_err(crate::control_error)?;
            emit_summary(&summary, json)
        }
        BrowserTargetCommand::Snapshot { interactive } => {
            let snapshot =
                live::browser_snapshot(target, interactive).map_err(crate::control_error)?;
            emit_snapshot(&snapshot, json)
        }
        BrowserTargetCommand::Click {
            element_ref,
            snapshot_after,
        } => act(element_ref, "click", None, snapshot_after),
        BrowserTargetCommand::Hover {
            element_ref,
            snapshot_after,
        } => act(element_ref, "hover", None, snapshot_after),
        BrowserTargetCommand::Scroll {
            element_ref,
            snapshot_after,
        } => act(element_ref, "scroll", None, snapshot_after),
        BrowserTargetCommand::Fill {
            element_ref,
            value,
            snapshot_after,
        } => act(element_ref, "fill", Some(value), snapshot_after),
        BrowserTargetCommand::Press {
            element_ref,
            key,
            snapshot_after,
        } => act(element_ref, "press", Some(key), snapshot_after),
        BrowserTargetCommand::Select {
            element_ref,
            value,
            snapshot_after,
        } => act(element_ref, "select", Some(value), snapshot_after),
        BrowserTargetCommand::Screenshot { path, full_page } => {
            let result =
                live::browser_screenshot(target, &path, full_page).map_err(crate::control_error)?;
            if json {
                return json_envelope("screenshot", &result);
            }
            Ok(format!("wrote {}\n", result.path))
        }
        BrowserTargetCommand::Viewport { width, height } => {
            let (width, height, reset) = parse_viewport_args(width.as_deref(), height)?;
            let summary = live::browser_viewport(target, width, height, reset)
                .map_err(crate::control_error)?;
            emit_summary(&summary, json)
        }
        BrowserTargetCommand::Eval { expression } => {
            let value = live::browser_eval(target, &expression).map_err(crate::control_error)?;
            if json {
                return json_envelope("value", &value);
            }
            Ok(format!(
                "{}\n",
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string())
            ))
        }
        BrowserTargetCommand::Console => {
            let entries = live::browser_console(target).map_err(crate::control_error)?;
            emit_console(&entries, json)
        }
    }
}

/// Reads `viewport <width> <height>` or `viewport reset`.
pub fn parse_viewport_args(
    width: Option<&str>,
    height: Option<u32>,
) -> Result<(Option<u32>, Option<u32>, bool), CliError> {
    match (width, height) {
        (Some("reset"), None) => Ok((None, None, true)),
        (Some("reset"), Some(_)) => Err(CliError::generic(
            "viewport reset does not take a height",
        )),
        (Some(width), Some(height)) => {
            let parsed: u32 = width.parse().map_err(|_| {
                CliError::generic(format!("{width} is not a viewport width in pixels"))
            })?;
            if parsed == 0 || height == 0 {
                return Err(CliError::generic(
                    "viewport width and height must both be greater than zero",
                ));
            }
            Ok((Some(parsed), Some(height), false))
        }
        _ => Err(CliError::backend(
            ExitCode::Generic,
            "browser_invalid_request",
            "viewport needs a width and a height, or `reset`",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn splits_a_target_from_its_verb() {
        let (target, rest) = split_target_invocation(&tokens(&["browser:2", "click", "e3"]))
            .expect("split");
        assert_eq!(target, "browser:2");
        assert_eq!(rest, tokens(&["click", "e3"]));
    }

    #[test]
    fn a_missing_target_says_what_the_call_should_look_like() {
        let error = split_target_invocation(&[]).expect_err("no target");
        assert!(error.message.contains("wardian browser browser:1 reload"));
    }

    #[test]
    fn a_target_with_no_verb_lists_examples_rather_than_failing_opaquely() {
        let error = split_target_invocation(&tokens(&["browser:1"])).expect_err("no verb");
        assert!(error.message.contains("needs an action"));
        assert!(error.message.contains("click e2"));
    }

    #[test]
    fn a_blank_target_is_treated_as_missing() {
        assert!(split_target_invocation(&tokens(&["   ", "reload"])).is_err());
    }

    #[test]
    fn target_verbs_parse_from_the_remaining_tokens() {
        let parsed = BrowserTargetArgs::try_parse_from(tokens(&["click", "e2", "--snapshot-after"]))
            .expect("parse");
        match parsed.command {
            BrowserTargetCommand::Click {
                element_ref,
                snapshot_after,
            } => {
                assert_eq!(element_ref, "e2");
                assert!(snapshot_after);
            }
            other => panic!("expected a click, got {other:?}"),
        }
    }

    #[test]
    fn wait_flags_parse_into_their_options() {
        let parsed = BrowserTargetArgs::try_parse_from(tokens(&[
            "wait",
            "--selector",
            "#ready",
            "--timeout-ms",
            "2500",
        ]))
        .expect("parse");
        match parsed.command {
            BrowserTargetCommand::Wait {
                selector,
                timeout_ms,
                load_state,
                ..
            } => {
                assert_eq!(selector.as_deref(), Some("#ready"));
                assert_eq!(timeout_ms, Some(2500));
                assert_eq!(load_state, None);
            }
            other => panic!("expected a wait, got {other:?}"),
        }
    }

    #[test]
    fn viewport_accepts_a_size_or_a_reset() {
        assert_eq!(
            parse_viewport_args(Some("800"), Some(600)).expect("size"),
            (Some(800), Some(600), false)
        );
        assert_eq!(
            parse_viewport_args(Some("reset"), None).expect("reset"),
            (None, None, true)
        );
    }

    #[test]
    fn viewport_refuses_a_partial_or_zero_size() {
        assert!(parse_viewport_args(Some("800"), None).is_err());
        assert!(parse_viewport_args(None, Some(600)).is_err());
        assert!(parse_viewport_args(Some("0"), Some(600)).is_err());
        assert!(parse_viewport_args(Some("800"), Some(0)).is_err());
        assert!(parse_viewport_args(Some("wide"), Some(600)).is_err());
        assert!(parse_viewport_args(Some("reset"), Some(600)).is_err());
    }

    #[test]
    fn json_output_is_wrapped_in_the_standard_envelope() {
        let rendered = json_envelope("session", &serde_json::json!({ "short_ref": "browser:1" }))
            .expect("json");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("parse");
        assert_eq!(parsed["schema"], 1);
        assert_eq!(parsed["session"]["short_ref"], "browser:1");
    }

    #[test]
    fn an_empty_console_says_so_instead_of_printing_nothing() {
        assert!(emit_console(&[], false).expect("render").contains("no console messages"));
        let json = emit_console(&[], true).expect("render");
        assert!(json.contains("\"console\""));
    }

    #[test]
    fn console_entries_render_one_line_each() {
        let entries = vec![
            ConsoleEntry {
                level: "error".to_string(),
                text: "boom".to_string(),
            },
            ConsoleEntry {
                level: "info".to_string(),
                text: "ok".to_string(),
            },
        ];
        let rendered = emit_console(&entries, false).expect("render");
        assert_eq!(rendered.lines().count(), 2);
        assert!(rendered.starts_with("error  boom"));
    }

    #[test]
    fn an_action_without_a_snapshot_prints_only_the_verb_and_ref() {
        let result = BrowserActionResult {
            browser_id: "b1".to_string(),
            action: "click".to_string(),
            element_ref: "e4".to_string(),
            snapshot: None,
        };
        assert_eq!(emit_action(&result, false).expect("render"), "click e4\n");
    }

    #[test]
    fn an_action_with_snapshot_after_prints_the_new_refs() {
        let result = BrowserActionResult {
            browser_id: "b1".to_string(),
            action: "click".to_string(),
            element_ref: "e4".to_string(),
            snapshot: Some(PageSnapshot {
                generation: 3,
                url: "https://example.com/next".to_string(),
                title: "Next".to_string(),
                interactive_only: true,
                truncated: false,
                elements: Vec::new(),
            }),
        };
        let rendered = emit_action(&result, false).expect("render");
        assert!(rendered.starts_with("click e4\n"));
        assert!(rendered.contains("generation: 3"));
        assert!(rendered.contains("https://example.com/next"));
    }
}

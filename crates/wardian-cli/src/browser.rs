//! `wardian browser` — the one interface humans and agents both use.
//!
//! Sessions are addressed as `browser:N` (short ref) or by id, matching how
//! the workbench labels them. Every subcommand supports `--json` so an agent
//! can branch on structure instead of parsing prose.

use clap::Parser;

use crate::args::{
    BrowserArgs, BrowserCommand, BrowserCookieCommand, BrowserStorageCommand, BrowserTargetArgs,
    BrowserTargetCommand,
};
use crate::errors::{CliError, ExitCode};
use crate::live;
use wardian_core::browser::{
    render_cookie_line, render_download_line, render_network_detail, render_network_line,
    render_session_line, render_snapshot, render_storage, BrowserActionResult,
    BrowserSessionSummary, ConsoleEntry, CookieAction, NetworkAction, NetworkFilter,
    NetworkOutcome, PageSnapshot, StatusFilter, StorageAction, StorageArea, StorageOutcome,
};

/// The working directory as a path string, when it can be read.
///
/// Not an error when it cannot: a session with no workspace still opens, it
/// just has nothing to guess an address from.
fn working_directory() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
}

/// Serializes a response under the CLI's standard envelope.
fn json_envelope<T: serde::Serialize>(key: &str, value: &T) -> Result<String, CliError> {
    let envelope = serde_json::json!({ "schema": 1, key: value });
    serde_json::to_string(&envelope)
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
            CliError::generic(
                "browser needs a session, e.g. `wardian browser browser:1 navigate reload`",
            )
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
            blank,
        } => {
            // An agent that does not name an owner owns what it opens, so its
            // sessions are closed with it rather than outliving it.
            let owner = agent.or_else(live::current_session_id);
            // An agent runs this from its own workspace, which is where the dev
            // server it wants to look at is running. Defaulting to the working
            // directory is what makes `wardian browser open` with no arguments
            // land on the right page instead of a blank one.
            let workspace = workspace.or_else(working_directory);
            let summary = live::browser_open(url, owner, workspace, width, height, detached, blank)
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
            let parsed = match BrowserTargetArgs::try_parse_from(rest) {
                Ok(parsed) => parsed,
                Err(error)
                    if matches!(
                        error.kind(),
                        clap::error::ErrorKind::DisplayHelp
                            | clap::error::ErrorKind::DisplayVersion
                    ) =>
                {
                    return Ok(error.to_string())
                }
                Err(error) => return Err(crate::parse_error(error)),
            };
            handle_target(&target, parsed.command, json || parsed.json)
        }
    }
}

/// Resolve nested help before the CLI's compatibility migrations run.
pub fn target_help(args: &BrowserArgs) -> Option<String> {
    let BrowserCommand::Target(tokens) = &args.command else {
        return None;
    };
    let (_, rest) = split_target_invocation(tokens).ok()?;
    let error = BrowserTargetArgs::try_parse_from(rest).err()?;
    matches!(
        error.kind(),
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
    )
    .then(|| error.to_string())
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
            let result =
                live::browser_get(target, &field, selector).map_err(crate::control_error)?;
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
        BrowserTargetCommand::Console { level, clear } => {
            let entries =
                live::browser_console(target, level, clear).map_err(crate::control_error)?;
            emit_console(&entries, json)
        }
        BrowserTargetCommand::Network {
            request_id,
            body,
            filter,
            method,
            status,
            resource_type,
            failed,
            limit,
            clear,
        } => {
            let action = network_action(
                request_id,
                body,
                filter,
                method,
                status.as_deref(),
                resource_type.as_deref(),
                failed,
                limit,
                clear,
            )?;
            let value = live::browser_network(target, action).map_err(crate::control_error)?;
            let outcome: NetworkOutcome = serde_json::from_value(value)
                .map_err(|error| CliError::generic(error.to_string()))?;
            emit_network(&outcome, json)
        }
        BrowserTargetCommand::Cookies { command, all } => {
            let action = cookie_action(command, all)?;
            let listing = matches!(action, CookieAction::List { .. });
            let cookies = live::browser_cookies(target, action).map_err(crate::control_error)?;
            if json {
                return json_envelope("cookies", &cookies);
            }
            if !listing {
                return Ok("ok\n".to_string());
            }
            if cookies.is_empty() {
                return Ok("no cookies\n".to_string());
            }
            Ok(cookies
                .iter()
                .map(render_cookie_line)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n")
        }
        BrowserTargetCommand::Storage { area, command } => {
            let (area, action) = storage_action(&area, command)?;
            let value =
                live::browser_storage(target, area, action).map_err(crate::control_error)?;
            let outcome: StorageOutcome = serde_json::from_value(value)
                .map_err(|error| CliError::generic(error.to_string()))?;
            emit_storage(&outcome, json)
        }
        BrowserTargetCommand::Downloads { clear } => {
            let records = live::browser_downloads(target, clear).map_err(crate::control_error)?;
            if json {
                return json_envelope("downloads", &records);
            }
            if records.is_empty() {
                return Ok("no downloads\n".to_string());
            }
            Ok(records
                .iter()
                .map(render_download_line)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n")
        }
    }
}

/// Builds the network verb from its mutually shaped flags.
///
/// `--body` without a request id is refused by clap; everything else that can
/// only be wrong at runtime is refused here rather than silently ignored.
#[allow(clippy::too_many_arguments)]
pub fn network_action(
    request_id: Option<String>,
    body: bool,
    filter: Option<String>,
    method: Option<String>,
    status: Option<&str>,
    resource_type: Option<&str>,
    failed: bool,
    limit: Option<usize>,
    clear: bool,
) -> Result<NetworkAction, CliError> {
    if clear {
        return Ok(NetworkAction::Clear);
    }
    if let Some(request_id) = request_id {
        return Ok(NetworkAction::Detail { request_id, body });
    }
    let status = match status {
        Some(status) => Some(StatusFilter::parse(status).ok_or_else(|| {
            CliError::generic(format!(
                "{status} is not a status; use an exact code like 404 or a class like 2xx"
            ))
        })?),
        None => None,
    };
    let resource_types = resource_type
        .map(|types| {
            types
                .split(',')
                .map(str::trim)
                .filter(|kind| !kind.is_empty())
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(NetworkAction::List {
        filter: NetworkFilter {
            text: filter,
            method,
            status,
            resource_types,
            failed_only: failed,
            limit,
        },
    })
}

/// Builds the cookie verb, refusing `--all` on anything but a listing.
pub fn cookie_action(
    command: Option<BrowserCookieCommand>,
    all: bool,
) -> Result<CookieAction, CliError> {
    match command {
        None => Ok(CookieAction::List { all }),
        Some(_) if all => Err(CliError::generic("--all only applies to listing cookies")),
        Some(BrowserCookieCommand::Set {
            name,
            value,
            url,
            domain,
            path,
            secure,
            http_only,
            same_site,
            expires,
        }) => Ok(CookieAction::Set {
            name,
            value,
            url,
            domain,
            path,
            secure,
            http_only,
            same_site,
            expires,
        }),
        Some(BrowserCookieCommand::Delete {
            name,
            url,
            domain,
            path,
        }) => Ok(CookieAction::Delete {
            name,
            url,
            domain,
            path,
        }),
        Some(BrowserCookieCommand::Clear) => Ok(CookieAction::Clear),
    }
}

/// Builds the storage verb, including the bare-key form clap cannot express.
///
/// `storage local token` reads one key. clap sees `token` as an unrecognized
/// subcommand, so it arrives as an external subcommand and is unpacked here.
pub fn storage_action(
    area: &str,
    command: Option<BrowserStorageCommand>,
) -> Result<(StorageArea, StorageAction), CliError> {
    let parsed = StorageArea::parse(area).ok_or_else(|| {
        CliError::generic(format!(
            "{area} is not a storage area; use local or session"
        ))
    })?;
    let action = match command {
        None => StorageAction::Get { key: None },
        Some(BrowserStorageCommand::Get { key }) => StorageAction::Get { key: Some(key) },
        Some(BrowserStorageCommand::Set { key, value }) => StorageAction::Set { key, value },
        Some(BrowserStorageCommand::Remove { key }) => StorageAction::Remove { key },
        Some(BrowserStorageCommand::Clear) => StorageAction::Clear,
        Some(BrowserStorageCommand::Key(tokens)) => {
            let mut tokens = tokens.into_iter();
            let key = tokens.next().unwrap_or_default();
            if key.is_empty() {
                return Err(CliError::generic("storage needs a key or a verb"));
            }
            if tokens.next().is_some() {
                return Err(CliError::generic(format!(
                    "`storage {area} {key} …` takes no extra arguments; use `set {key} <value>` to write"
                )));
            }
            StorageAction::Get { key: Some(key) }
        }
    };
    Ok((parsed, action))
}

fn emit_network(outcome: &NetworkOutcome, json: bool) -> Result<String, CliError> {
    if json {
        return json_envelope("network", outcome);
    }
    match outcome {
        NetworkOutcome::Cleared => Ok("cleared the network ledger\n".to_string()),
        NetworkOutcome::Detail { detail } => Ok(format!("{}\n", render_network_detail(detail))),
        NetworkOutcome::List { entries } if entries.is_empty() => {
            Ok("no requests match\n".to_string())
        }
        NetworkOutcome::List { entries } => Ok(entries
            .iter()
            .map(render_network_line)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"),
    }
}

fn emit_storage(outcome: &StorageOutcome, json: bool) -> Result<String, CliError> {
    if json {
        return json_envelope("storage", outcome);
    }
    match outcome {
        StorageOutcome::Applied => Ok("ok\n".to_string()),
        StorageOutcome::Value { value: None } => Ok("(not set)\n".to_string()),
        StorageOutcome::Value { value: Some(value) } => Ok(format!("{value}\n")),
        StorageOutcome::Snapshot { snapshot } => Ok(format!("{}\n", render_storage(snapshot))),
    }
}

/// Reads `viewport <width> <height>` or `viewport reset`.
pub fn parse_viewport_args(
    width: Option<&str>,
    height: Option<u32>,
) -> Result<(Option<u32>, Option<u32>, bool), CliError> {
    match (width, height) {
        (Some("reset"), None) => Ok((None, None, true)),
        (Some("reset"), Some(_)) => Err(CliError::generic("viewport reset does not take a height")),
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
        let (target, rest) =
            split_target_invocation(&tokens(&["browser:2", "click", "e3"])).expect("split");
        assert_eq!(target, "browser:2");
        assert_eq!(rest, tokens(&["click", "e3"]));
    }

    #[test]
    fn a_missing_target_says_what_the_call_should_look_like() {
        let error = split_target_invocation(&[]).expect_err("no target");
        assert!(error
            .message
            .contains("wardian browser browser:1 navigate reload"));
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
        let parsed =
            BrowserTargetArgs::try_parse_from(tokens(&["click", "e2", "--snapshot-after"]))
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
        assert!(emit_console(&[], false)
            .expect("render")
            .contains("no console messages"));
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

    /// Parses a full `wardian browser <target> …` tail the way the CLI does.
    fn target(values: &[&str]) -> BrowserTargetCommand {
        BrowserTargetArgs::try_parse_from(tokens(values))
            .expect("parse")
            .command
    }

    #[test]
    fn console_takes_a_level_and_a_clear_flag() {
        match target(&["console", "--level", "error", "--clear"]) {
            BrowserTargetCommand::Console { level, clear } => {
                assert_eq!(level.as_deref(), Some("error"));
                assert!(clear);
            }
            other => panic!("expected console, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_network_call_lists_everything() {
        match target(&["network"]) {
            BrowserTargetCommand::Network {
                request_id,
                clear,
                filter,
                ..
            } => assert!(request_id.is_none() && !clear && filter.is_none()),
            other => panic!("expected network, got {other:?}"),
        }
        let action = network_action(None, false, None, None, None, None, false, None, false)
            .expect("action");
        assert_eq!(
            action,
            NetworkAction::List {
                filter: NetworkFilter::default()
            }
        );
    }

    #[test]
    fn network_flags_become_one_filter() {
        let action = network_action(
            None,
            false,
            Some("api".to_string()),
            Some("POST".to_string()),
            Some("2xx"),
            Some("xhr, Fetch ,"),
            true,
            Some(25),
            false,
        )
        .expect("action");
        match action {
            NetworkAction::List { filter } => {
                assert_eq!(filter.text.as_deref(), Some("api"));
                assert_eq!(filter.method.as_deref(), Some("POST"));
                assert_eq!(filter.status, Some(StatusFilter::Class(2)));
                assert_eq!(filter.resource_types, vec!["xhr", "fetch"]);
                assert!(filter.failed_only);
                assert_eq!(filter.limit, Some(25));
            }
            other => panic!("expected a listing, got {other:?}"),
        }
    }

    #[test]
    fn an_unreadable_status_names_both_accepted_forms() {
        let error = network_action(
            None,
            false,
            None,
            None,
            Some("okay"),
            None,
            false,
            None,
            false,
        )
        .expect_err("rejected");
        assert!(error.message.contains("404"));
        assert!(error.message.contains("2xx"));
    }

    #[test]
    fn a_request_id_selects_the_detail_view_and_carries_the_body_flag() {
        let action = network_action(
            Some("42.1".to_string()),
            true,
            None,
            None,
            None,
            None,
            false,
            None,
            false,
        )
        .expect("action");
        assert_eq!(
            action,
            NetworkAction::Detail {
                request_id: "42.1".to_string(),
                body: true
            }
        );
    }

    #[test]
    fn clear_wins_over_everything_it_is_allowed_beside() {
        let action =
            network_action(None, false, None, None, None, None, false, None, true).expect("action");
        assert_eq!(action, NetworkAction::Clear);
    }

    #[test]
    fn network_refuses_a_body_read_without_a_request_to_read_it_from() {
        assert!(BrowserTargetArgs::try_parse_from(tokens(&["network", "--body"])).is_err());
    }

    #[test]
    fn network_refuses_clear_alongside_a_filter() {
        assert!(BrowserTargetArgs::try_parse_from(tokens(&[
            "network", "--clear", "--filter", "api"
        ]))
        .is_err());
    }

    #[test]
    fn a_bare_cookies_call_lists_the_pages_cookies() {
        assert_eq!(
            cookie_action(None, false).expect("action"),
            CookieAction::List { all: false }
        );
        assert_eq!(
            cookie_action(None, true).expect("action"),
            CookieAction::List { all: true }
        );
    }

    #[test]
    fn cookie_set_carries_every_attribute_it_was_given() {
        match target(&[
            "cookies",
            "set",
            "sid",
            "abc",
            "--secure",
            "--http-only",
            "--same-site",
            "lax",
            "--expires",
            "1800000000",
        ]) {
            BrowserTargetCommand::Cookies { command, all } => assert_eq!(
                cookie_action(command, all).expect("action"),
                CookieAction::Set {
                    name: "sid".to_string(),
                    value: "abc".to_string(),
                    url: None,
                    domain: None,
                    path: None,
                    secure: true,
                    http_only: true,
                    same_site: Some("lax".to_string()),
                    expires: Some(1_800_000_000),
                }
            ),
            other => panic!("expected cookies, got {other:?}"),
        }
    }

    #[test]
    fn all_is_refused_on_a_cookie_mutation_rather_than_silently_ignored() {
        let error = cookie_action(Some(BrowserCookieCommand::Clear), true).expect_err("rejected");
        assert!(error.message.contains("only applies to listing"));
    }

    #[test]
    fn a_bare_storage_area_lists_the_whole_area() {
        let (area, action) = storage_action("local", None).expect("action");
        assert_eq!(area, StorageArea::Local);
        assert_eq!(action, StorageAction::Get { key: None });
    }

    #[test]
    fn a_bare_key_reads_that_key() {
        match target(&["storage", "session", "token"]) {
            BrowserTargetCommand::Storage { area, command } => {
                let (area, action) = storage_action(&area, command).expect("action");
                assert_eq!(area, StorageArea::Session);
                assert_eq!(
                    action,
                    StorageAction::Get {
                        key: Some("token".to_string())
                    }
                );
            }
            other => panic!("expected storage, got {other:?}"),
        }
    }

    #[test]
    fn a_bare_key_with_a_stray_value_points_at_set_instead_of_guessing() {
        match target(&["storage", "local", "token", "abc"]) {
            BrowserTargetCommand::Storage { area, command } => {
                let error = storage_action(&area, command).expect_err("rejected");
                assert!(error.message.contains("set token <value>"));
            }
            other => panic!("expected storage, got {other:?}"),
        }
    }

    #[test]
    fn storage_verbs_parse_into_their_actions() {
        for (values, expected) in [
            (
                vec!["storage", "local", "set", "theme", "dark"],
                StorageAction::Set {
                    key: "theme".to_string(),
                    value: "dark".to_string(),
                },
            ),
            (
                vec!["storage", "session", "remove", "theme"],
                StorageAction::Remove {
                    key: "theme".to_string(),
                },
            ),
            (vec!["storage", "local", "clear"], StorageAction::Clear),
        ] {
            match target(&values) {
                BrowserTargetCommand::Storage { area, command } => {
                    let (_, action) = storage_action(&area, command).expect("action");
                    assert_eq!(action, expected, "{values:?}");
                }
                other => panic!("expected storage, got {other:?}"),
            }
        }
    }

    #[test]
    fn an_unknown_storage_area_names_the_two_that_exist() {
        let error = storage_action("cookies", None).expect_err("rejected");
        assert!(error.message.contains("local or session"));
    }

    #[test]
    fn a_network_listing_renders_one_line_per_request() {
        let outcome = NetworkOutcome::List {
            entries: vec![wardian_core::browser::NetworkEntry {
                request_id: "1".to_string(),
                method: "GET".to_string(),
                url: "https://example.com/api".to_string(),
                resource_type: "xhr".to_string(),
                status: Some(200),
                mime_type: None,
                encoded_data_length: None,
                failure: None,
                from_cache: false,
                duration_ms: None,
                url_truncated: false,
            }],
        };
        let text = emit_network(&outcome, false).expect("render");
        assert!(text.contains("https://example.com/api"));
        assert!(text.ends_with('\n'));

        let empty = NetworkOutcome::List {
            entries: Vec::new(),
        };
        assert_eq!(
            emit_network(&empty, false).expect("render"),
            "no requests match\n"
        );
        assert_eq!(
            emit_network(&NetworkOutcome::Cleared, false).expect("render"),
            "cleared the network ledger\n"
        );
    }

    #[test]
    fn json_output_wraps_every_shape_in_the_standard_envelope() {
        let text = emit_network(&NetworkOutcome::Cleared, true).expect("render");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["schema"], 1);
        assert_eq!(value["network"]["outcome"], "cleared");

        let text = emit_storage(&StorageOutcome::Applied, true).expect("render");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["storage"]["outcome"], "applied");
    }

    #[test]
    fn a_storage_key_that_is_not_set_says_so_rather_than_printing_a_blank_line() {
        assert_eq!(
            emit_storage(&StorageOutcome::Value { value: None }, false).expect("render"),
            "(not set)\n"
        );
        assert_eq!(
            emit_storage(
                &StorageOutcome::Value {
                    value: Some("dark".to_string())
                },
                false
            )
            .expect("render"),
            "dark\n"
        );
    }

    #[test]
    fn open_refuses_blank_alongside_an_address() {
        // Asking for a page and for no page is not a preference to resolve.
        let parsed = crate::args::Cli::try_parse_from(tokens(&[
            "wardian",
            "browser",
            "open",
            "https://example.com/",
            "--blank",
        ]));
        assert!(parsed.is_err());
    }

    #[test]
    fn open_accepts_blank_on_its_own() {
        let parsed =
            crate::args::Cli::try_parse_from(tokens(&["wardian", "browser", "open", "--blank"]))
                .expect("parse");
        match parsed.command {
            crate::args::Command::Browser(args) => match args.command {
                BrowserCommand::Open { url, blank, .. } => {
                    assert!(url.is_none());
                    assert!(blank);
                }
                other => panic!("expected open, got {other:?}"),
            },
            _ => panic!("expected the browser command"),
        }
    }

    #[test]
    fn the_working_directory_stands_in_for_an_unnamed_workspace() {
        // An agent runs this from its own workspace, which is where the dev
        // server it wants to look at is running.
        let directory = working_directory().expect("a working directory");
        assert!(!directory.is_empty());
        assert_eq!(
            std::path::Path::new(&directory),
            std::env::current_dir().expect("cwd"),
        );
    }

    #[test]
    fn downloads_parse_their_clear_flag() {
        match target(&["downloads", "--clear"]) {
            BrowserTargetCommand::Downloads { clear } => assert!(clear),
            other => panic!("expected downloads, got {other:?}"),
        }
    }
}

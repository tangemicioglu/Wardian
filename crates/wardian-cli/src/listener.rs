//! `wardian automation listener` — manage event listeners from the CLI.
//!
//! The CLI writes the same locked config file the app reads, so a listener can
//! be created with the app closed; the supervisor arms it on its next
//! reconcile. Every mutation runs the same core validation the Tauri commands
//! do, so the two surfaces cannot drift into accepting different configs.

use crate::args::{AutomationListenerCommand, ListenerCommonArgs};
use crate::automation_invoker::{
    parse_automation_bindings, parse_automation_exec_input, validate_schedule_blueprint,
    validate_schedule_provider,
};
use crate::errors::{CliError, ExitCode};
use crate::json_input;
use crate::render_json;
use std::collections::BTreeMap;
use wardian_core::listeners::{
    self, secrets, webhook as webhook_rules, AutomationListener, FileChangeKind, FileWatchTrigger,
    FingerprintSource, ListenerTrigger, OverlapPolicy, PollMethod, WebPollTrigger, WebhookAuth,
    WebhookTrigger, DEFAULT_DEBOUNCE_MS, DEFAULT_POLL_MAX_BODY_BYTES,
    DEFAULT_WEBHOOK_MAX_BODY_BYTES, MIN_POLL_INTERVAL_SECONDS,
};
use wardian_core::models::AutomationAssignments;

fn not_found(id: &str) -> CliError {
    let mut error = CliError::backend(
        ExitCode::NotFound,
        "listener_not_found",
        format!("listener not found: {id}"),
    );
    error.hint = Some("Run `wardian automation listener list` to see listener ids.".to_string());
    error
}

fn parse_overlap(value: Option<&str>) -> Result<Option<OverlapPolicy>, CliError> {
    match value {
        None => Ok(None),
        Some("skip") => Ok(Some(OverlapPolicy::Skip)),
        Some("coalesce") => Ok(Some(OverlapPolicy::Coalesce)),
        Some("parallel") => Ok(Some(OverlapPolicy::Parallel)),
        Some(other) => Err(CliError::generic(format!(
            "unsupported overlap `{other}`; expected skip, coalesce, or parallel"
        ))),
    }
}

fn parse_change_kinds(values: &[String]) -> Result<Vec<FileChangeKind>, CliError> {
    values
        .iter()
        .map(|value| match value.to_ascii_lowercase().as_str() {
            "created" | "create" => Ok(FileChangeKind::Created),
            "modified" | "modify" => Ok(FileChangeKind::Modified),
            "removed" | "remove" | "deleted" => Ok(FileChangeKind::Removed),
            other => Err(CliError::generic(format!(
                "unsupported --event `{other}`; expected created, modified, or removed"
            ))),
        })
        .collect()
}

/// Parse `--header 'Name: value'` pairs.
fn parse_headers(values: &[String]) -> Result<BTreeMap<String, String>, CliError> {
    let mut headers = BTreeMap::new();
    for value in values {
        let (name, content) = value.split_once(':').ok_or_else(|| {
            CliError::generic(format!(
                "invalid --header `{value}`; expected `Name: value`"
            ))
        })?;
        headers.insert(name.trim().to_string(), content.trim().to_string());
    }
    Ok(headers)
}

struct CommonContext {
    provider: Option<String>,
    workspace: Option<String>,
    input: serde_json::Value,
    bindings: std::collections::HashMap<String, String>,
    assignments: AutomationAssignments,
    overlap: Option<OverlapPolicy>,
    enabled: bool,
}

fn resolve_common(common: &ListenerCommonArgs) -> Result<CommonContext, CliError> {
    let input = parse_automation_exec_input(common.input.as_deref())?;
    let mut assignments: AutomationAssignments = common
        .assignments
        .as_deref()
        .map(|raw| json_input::parse(raw, "--assignments"))
        .transpose()?
        .unwrap_or_default();
    let legacy = parse_automation_bindings(&common.bind)?;
    assignments = wardian_core::automation::assignment::normalize_assignments(
        Some(assignments),
        &legacy,
        wardian_core::models::InvocationKind::Listener,
    );
    wardian_core::automation::assignment::validate_assignments(&assignments)
        .map_err(CliError::generic)?;
    let bindings = wardian_core::automation::assignment::legacy_bindings(&assignments);
    Ok(CommonContext {
        provider: common.provider.clone(),
        workspace: common.workspace.clone(),
        input,
        bindings,
        assignments,
        overlap: parse_overlap(common.overlap.as_deref())?,
        enabled: common.enable,
    })
}

fn build(
    blueprint_id: String,
    name: String,
    trigger: ListenerTrigger,
    context: CommonContext,
) -> AutomationListener {
    AutomationListener {
        id: wardian_core::engine::driver::new_run_id(),
        blueprint_id,
        name,
        enabled: context.enabled,
        trigger,
        provider: context.provider,
        workspace: context.workspace,
        input: context.input,
        bindings: context.bindings,
        assignments: context.assignments,
        overlap: context.overlap,
        runtime: Default::default(),
    }
}

/// Validate, refuse a duplicate webhook path, and persist.
fn persist(listener: &AutomationListener) -> Result<(), CliError> {
    listeners::validate_listener(listener).map_err(CliError::generic)?;
    let existing = listeners::load_listeners();
    if let ListenerTrigger::Webhook(trigger) = &listener.trigger {
        webhook_rules::ensure_unique_path(&existing, &listener.id, &trigger.path_segment)
            .map_err(CliError::generic)?;
    }
    let record = listener.clone();
    listeners::mutate_listeners(|stored| {
        stored.push(record.clone());
        Ok(())
    })
    .map_err(|error| CliError::generic(error.to_string()))
}

/// Render a listener with the derived facts a user needs, and never the secret.
fn describe(listener: &AutomationListener) -> serde_json::Value {
    let mut value = serde_json::to_value(listener).unwrap_or(serde_json::Value::Null);
    if let (Some(object), ListenerTrigger::Webhook(trigger)) =
        (value.as_object_mut(), &listener.trigger)
    {
        object.insert(
            "webhook_url".into(),
            serde_json::Value::String(webhook_rules::webhook_url(
                &webhook_rules::load_gateway_config(),
                &trigger.path_segment,
            )),
        );
    }
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "has_secret".into(),
            serde_json::Value::Bool(
                secrets::load_secret(&listener.id).is_some_and(|stored| !stored.is_empty()),
            ),
        );
        object.insert(
            "trigger_type".into(),
            serde_json::Value::String(listener.trigger.kind().to_string()),
        );
    }
    value
}

fn mutate_one(
    id: &str,
    mutate: impl FnOnce(&mut AutomationListener),
) -> Result<AutomationListener, CliError> {
    let mut missing = false;
    let result = listeners::mutate_listeners(|stored| {
        match stored.iter_mut().find(|listener| listener.id == id) {
            Some(listener) => {
                mutate(listener);
                Ok(Some(listener.clone()))
            }
            None => {
                missing = true;
                Ok(None)
            }
        }
    })
    .map_err(|error| CliError::generic(error.to_string()))?;
    result.ok_or_else(|| not_found(id))
}

pub fn render(command: AutomationListenerCommand) -> Result<String, CliError> {
    use AutomationListenerCommand as C;

    match command {
        C::List => {
            let listeners: Vec<serde_json::Value> =
                listeners::load_listeners().iter().map(describe).collect();
            render_json(serde_json::json!({ "schema": 1, "listeners": listeners }))
        }
        C::Show { id } => {
            let listener = listeners::load_listeners()
                .into_iter()
                .find(|listener| listener.id == id)
                .ok_or_else(|| not_found(&id))?;
            render_json(serde_json::json!({ "schema": 1, "listener": describe(&listener) }))
        }
        C::Watch {
            blueprint,
            name,
            path,
            recursive,
            pattern,
            ignore,
            event,
            debounce_ms,
            common,
        } => {
            validate_schedule_blueprint(&blueprint)?;
            validate_schedule_provider(common.provider.as_deref())?;
            let context = resolve_common(&common)?;
            let trigger = ListenerTrigger::FileWatch(FileWatchTrigger {
                path,
                recursive,
                patterns: pattern,
                ignore,
                events: parse_change_kinds(&event)?,
                debounce_ms: debounce_ms.unwrap_or(DEFAULT_DEBOUNCE_MS),
            });
            let listener = build(blueprint, name, trigger, context);
            persist(&listener)?;
            render_json(serde_json::json!({ "schema": 1, "listener": describe(&listener) }))
        }
        C::Hook {
            blueprint,
            name,
            path,
            auth,
            signature_header,
            secret,
            max_body_bytes,
            common,
        } => {
            validate_schedule_blueprint(&blueprint)?;
            validate_schedule_provider(common.provider.as_deref())?;
            let auth = match auth.to_ascii_lowercase().as_str() {
                "token" => WebhookAuth::Token,
                "hmac" | "hmac_sha256" | "hmac-sha256" => WebhookAuth::HmacSha256,
                other => {
                    return Err(CliError::generic(format!(
                        "unsupported --auth `{other}`; expected token or hmac"
                    )))
                }
            };
            let context = resolve_common(&common)?;
            let trigger = ListenerTrigger::Webhook(WebhookTrigger {
                path_segment: path,
                auth,
                signature_header,
                max_body_bytes: max_body_bytes.unwrap_or(DEFAULT_WEBHOOK_MAX_BODY_BYTES),
            });
            let listener = build(blueprint, name, trigger, context);
            persist(&listener)?;

            // Returned once, in the clear, because HMAC verification needs the
            // raw value and the sender has to be configured with the same one.
            let secret = secret
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(secrets::generate_secret);
            let mut stored = secrets::load_secret(&listener.id).unwrap_or_default();
            stored.webhook_secret = Some(secret.clone());
            secrets::set_secret(&listener.id, stored)
                .map_err(|error| CliError::generic(error.to_string()))?;

            render_json(serde_json::json!({
                "schema": 1,
                "listener": describe(&listener),
                "secret": secret,
                "secret_note": "Shown once. Configure the sender with this value; Wardian stores it outside the listener config.",
            }))
        }
        C::Poll {
            blueprint,
            name,
            url,
            interval,
            method,
            fingerprint,
            json_pointer,
            regex,
            header,
            max_body_bytes,
            common,
        } => {
            validate_schedule_blueprint(&blueprint)?;
            validate_schedule_provider(common.provider.as_deref())?;
            let method = match method.to_ascii_lowercase().as_str() {
                "get" => PollMethod::Get,
                "head" => PollMethod::Head,
                other => {
                    return Err(CliError::generic(format!(
                        "unsupported --method `{other}`; expected get or head"
                    )))
                }
            };
            let fingerprint = match fingerprint.to_ascii_lowercase().as_str() {
                "etag" | "etag_or_last_modified" => FingerprintSource::EtagOrLastModified,
                "body" | "body_hash" => FingerprintSource::BodyHash,
                "json" | "json_pointer" => FingerprintSource::JsonPointer,
                "regex" => FingerprintSource::Regex,
                other => {
                    return Err(CliError::generic(format!(
                        "unsupported --fingerprint `{other}`; expected etag, body, json, or regex"
                    )))
                }
            };
            let context = resolve_common(&common)?;
            let trigger = ListenerTrigger::WebPoll(WebPollTrigger {
                url,
                interval_seconds: interval.unwrap_or(300).max(MIN_POLL_INTERVAL_SECONDS),
                method,
                headers: parse_headers(&header)?,
                fingerprint,
                json_pointer,
                regex,
                max_body_bytes: max_body_bytes.unwrap_or(DEFAULT_POLL_MAX_BODY_BYTES),
            });
            let listener = build(blueprint, name, trigger, context);
            persist(&listener)?;
            render_json(serde_json::json!({ "schema": 1, "listener": describe(&listener) }))
        }
        C::Enable { id } => {
            let listener = mutate_one(&id, |listener| {
                listener.enabled = true;
                // Clearing the auto-disable reason is the only way back from
                // the rate ceiling, and the reason the app never writes
                // `enabled` itself.
                listener.runtime.disabled_reason = None;
            })?;
            render_json(serde_json::json!({ "schema": 1, "listener": describe(&listener) }))
        }
        C::Disable { id } => {
            let listener = mutate_one(&id, |listener| listener.enabled = false)?;
            render_json(serde_json::json!({ "schema": 1, "listener": describe(&listener) }))
        }
        C::Remove { id } => {
            let mut removed = false;
            listeners::mutate_listeners(|stored| {
                let before = stored.len();
                stored.retain(|listener| listener.id != id);
                removed = stored.len() != before;
                Ok(())
            })
            .map_err(|error| CliError::generic(error.to_string()))?;
            if !removed {
                return Err(not_found(&id));
            }
            // Removing a listener must not leave a live credential behind.
            secrets::remove_secret(&id).map_err(|error| CliError::generic(error.to_string()))?;
            render_json(serde_json::json!({ "schema": 1, "removed": id }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_kinds_accept_both_tenses() {
        let parsed = parse_change_kinds(&["created".into(), "modify".into(), "deleted".into()])
            .expect("parse");
        assert_eq!(
            parsed,
            vec![
                FileChangeKind::Created,
                FileChangeKind::Modified,
                FileChangeKind::Removed
            ]
        );
        assert!(parse_change_kinds(&["renamed".into()]).is_err());
    }

    #[test]
    fn headers_split_on_the_first_colon_so_urls_survive() {
        let parsed = parse_headers(&["Accept: application/json".into()]).expect("parse");
        assert_eq!(
            parsed.get("Accept").map(String::as_str),
            Some("application/json")
        );

        let with_url = parse_headers(&["Referer: https://example.com/x".into()]).expect("parse");
        assert_eq!(
            with_url.get("Referer").map(String::as_str),
            Some("https://example.com/x")
        );
        assert!(parse_headers(&["no-colon".into()]).is_err());
    }

    #[test]
    fn overlap_parsing_rejects_an_unknown_policy() {
        assert_eq!(parse_overlap(None).expect("none"), None);
        assert_eq!(
            parse_overlap(Some("coalesce")).expect("parse"),
            Some(OverlapPolicy::Coalesce)
        );
        assert!(parse_overlap(Some("queue")).is_err());
    }

    #[test]
    fn describing_a_listener_never_carries_its_secret() {
        let listener = AutomationListener {
            id: "hook".into(),
            blueprint_id: "audit".into(),
            name: "CI".into(),
            enabled: true,
            trigger: ListenerTrigger::Webhook(WebhookTrigger {
                path_segment: "ci".into(),
                auth: WebhookAuth::HmacSha256,
                signature_header: None,
                max_body_bytes: DEFAULT_WEBHOOK_MAX_BODY_BYTES,
            }),
            provider: None,
            workspace: None,
            input: serde_json::json!({}),
            bindings: Default::default(),
            assignments: Default::default(),
            overlap: None,
            runtime: Default::default(),
        };
        let described = describe(&listener);
        let rendered = serde_json::to_string(&described).expect("render");

        assert_eq!(described["trigger_type"], "webhook");
        assert!(described["webhook_url"]
            .as_str()
            .expect("url")
            .ends_with("/hooks/ci"));
        assert!(described["has_secret"].is_boolean());
        assert!(
            !rendered.contains("webhook_secret"),
            "listener output must never carry the stored secret"
        );
    }
}

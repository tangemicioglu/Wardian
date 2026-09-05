//! Tauri commands for automation listener invokers.
//!
//! Every mutation goes through the same core validation the CLI uses, so the
//! two surfaces cannot drift into accepting different configurations.

use serde::Serialize;
use tauri::AppHandle;
use wardian_core::listeners::{
    self, secrets, webhook as webhook_rules, AutomationListener, ListenerTrigger,
};

/// A listener plus the derived facts a UI needs but must not store.
#[derive(Debug, Clone, Serialize)]
pub struct ListenerView {
    #[serde(flatten)]
    pub listener: AutomationListener,
    /// Where a sender should POST, for webhook listeners.
    pub webhook_url: Option<String>,
    /// Whether a secret exists, never the secret itself.
    pub has_secret: bool,
}

fn view(listener: AutomationListener) -> ListenerView {
    let webhook_url = match &listener.trigger {
        ListenerTrigger::Webhook(trigger) => Some(webhook_rules::webhook_url(
            &webhook_rules::load_gateway_config(),
            &trigger.path_segment,
        )),
        _ => None,
    };
    let has_secret = secrets::load_secret(&listener.id).is_some_and(|stored| !stored.is_empty());
    ListenerView {
        listener,
        webhook_url,
        has_secret,
    }
}

#[tauri::command]
pub async fn listener_list() -> Result<Vec<ListenerView>, String> {
    Ok(listeners::load_listeners().into_iter().map(view).collect())
}

/// Create or replace a listener.
///
/// Runtime state is deliberately preserved across an update rather than taken
/// from the caller: a UI round-trip must not be able to reset a poll
/// fingerprint or clear an auto-disable reason as a side effect of renaming.
#[tauri::command]
pub async fn listener_save(
    app: AppHandle,
    mut listener: AutomationListener,
) -> Result<ListenerView, String> {
    if listener.id.trim().is_empty() {
        listener.id = wardian_core::engine::driver::new_run_id();
    }
    wardian_core::automation::resolve_blueprint_path(&listener.blueprint_id)
        .ok_or_else(|| format!("automation blueprint not found: {}", listener.blueprint_id))?;
    listeners::validate_listener(&listener)?;

    // The uniqueness check and the runtime carry-over both run inside the lock.
    // Reading runtime outside it and writing the whole record back would lose
    // any fire, arming result, or poll fingerprint that landed in between,
    // silently resetting durable pacing and rate-ceiling state.
    let candidate = listener.clone();
    let saved = listeners::mutate_listeners(|stored| {
        if let ListenerTrigger::Webhook(trigger) = &candidate.trigger {
            webhook_rules::ensure_unique_path(stored, &candidate.id, &trigger.path_segment)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::AlreadyExists, error))?;
        }
        let mut record = candidate.clone();
        match stored.iter_mut().find(|item| item.id == record.id) {
            Some(existing) => {
                record.runtime = existing.runtime.clone();
                *existing = record.clone();
            }
            None => stored.push(record.clone()),
        }
        Ok(record)
    })
    .map_err(|error| error.to_string())?;

    crate::automation::listener::reconcile(&app).await;
    Ok(view(saved))
}

#[tauri::command]
pub async fn listener_delete(app: AppHandle, id: String) -> Result<(), String> {
    listeners::mutate_listeners(|stored| {
        let before = stored.len();
        stored.retain(|item| item.id != id);
        if stored.len() == before {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("listener not found: {id}"),
            ));
        }
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    // Removing the listener must not leave a live credential behind.
    let _ = secrets::remove_secret(&id);
    crate::automation::listener::reconcile(&app).await;
    Ok(())
}

/// Turn a listener on or off.
///
/// Enabling also clears an auto-disable reason, which is the only way back
/// from the rate ceiling and the reason the app never writes `enabled` itself.
#[tauri::command]
pub async fn listener_set_enabled(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    listeners::mutate_listeners(
        |stored| match stored.iter_mut().find(|item| item.id == id) {
            Some(listener) => {
                listener.enabled = enabled;
                if enabled {
                    listener.runtime.disabled_reason = None;
                }
                Ok(())
            }
            None => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("listener not found: {id}"),
            )),
        },
    )
    .map_err(|error| error.to_string())?;
    crate::automation::listener::reconcile(&app).await;
    Ok(())
}

/// Store a webhook secret, generating one when the caller supplies none.
///
/// Returns the secret exactly once, because HMAC verification needs it in the
/// clear and the sender has to be configured with the same value.
#[tauri::command]
pub async fn listener_set_webhook_secret(
    id: String,
    secret: Option<String>,
) -> Result<String, String> {
    let listeners = listeners::load_listeners();
    let listener = listeners
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| format!("listener not found: {id}"))?;
    if !matches!(listener.trigger, ListenerTrigger::Webhook(_)) {
        return Err("only webhook listeners take a webhook secret".to_string());
    }
    let secret = secret
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(secrets::generate_secret);

    let mut stored = secrets::load_secret(&id).unwrap_or_default();
    stored.webhook_secret = Some(secret.clone());
    secrets::set_secret(&id, stored).map_err(|error| error.to_string())?;
    Ok(secret)
}

/// Replace the credential-bearing request headers for a poll listener.
#[tauri::command]
pub async fn listener_set_poll_headers(
    id: String,
    headers: std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    let listeners = listeners::load_listeners();
    let listener = listeners
        .iter()
        .find(|item| item.id == id)
        .ok_or_else(|| format!("listener not found: {id}"))?;
    if !matches!(listener.trigger, ListenerTrigger::WebPoll(_)) {
        return Err("only web poll listeners take request headers".to_string());
    }
    let mut stored = secrets::load_secret(&id).unwrap_or_default();
    stored.headers = headers;
    secrets::set_secret(&id, stored).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn listener_gateway_config() -> Result<webhook_rules::WebhookGatewayConfig, String> {
    Ok(webhook_rules::load_gateway_config())
}

#[tauri::command]
pub async fn listener_gateway_save(
    app: AppHandle,
    config: webhook_rules::WebhookGatewayConfig,
) -> Result<webhook_rules::WebhookGatewayConfig, String> {
    webhook_rules::validate_gateway_config(&config)?;
    webhook_rules::save_gateway_config(&config).map_err(|error| error.to_string())?;
    crate::automation::listener::restart_webhook_gateway(&app).await;
    Ok(config)
}

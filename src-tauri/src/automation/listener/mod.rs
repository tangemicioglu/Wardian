//! The listener supervisor: reconciles persisted listener config into live
//! watchers, routes, and poll timers.
//!
//! A per-listener supervisor is not viable. The listener set is mutable at
//! runtime from two processes — the app through its commands and the CLI
//! through the same locked file — so only a single reconciler can guarantee
//! both "no orphaned watcher" and "no listener silently unarmed".

pub mod file;
pub mod launch;
pub mod poll;
pub mod webhook;

use launch::ListenerActivity;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use wardian_core::listeners::{self, AutomationListener, ListenerTrigger};

/// Reconcile cadence, matching the schedule invoker's tick so the two
/// unattended paths wake on the same rhythm.
const TICK_SECS: u64 = 5;

struct WebhookServer {
    handle: tauri::async_runtime::JoinHandle<()>,
}

/// Live listener state, managed on the app rather than folded into `AppState`
/// so the `notify` watchers it owns stay next to the code that arms them.
///
/// The watcher map uses a std mutex because `notify`'s watcher is not `Sync`,
/// the same accommodation `TopologyWatcherHandle` makes. It is never held
/// across an await.
#[derive(Default)]
pub struct ListenerSupervisor {
    pub activity: Mutex<ListenerActivity>,
    watchers: std::sync::Mutex<HashMap<String, file::FileWatch>>,
    webhook: Mutex<Option<WebhookServer>>,
    /// Fingerprint of the config that produced the current arming.
    fingerprint: Mutex<Option<String>>,
    tick: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

/// Arm or disarm everything so live state matches the persisted config.
///
/// Skips the whole rearm when only runtime fields changed. Without that check
/// every fire's runtime write-back would look like a config change and tear
/// down each watcher — a listener would disarm itself by working.
pub async fn reconcile(app: &AppHandle) {
    let listeners = listeners::load_listeners();
    let fingerprint = listeners::arming_fingerprint(&listeners);
    let supervisor = app.state::<ListenerSupervisor>();
    {
        let mut stored = supervisor.fingerprint.lock().await;
        if stored.as_deref() == Some(fingerprint.as_str()) {
            return;
        }
        *stored = Some(fingerprint);
    }

    let mut arm_results: Vec<(String, bool, Option<String>)> = Vec::new();

    // ---- file watchers ----
    let mut watchers = HashMap::new();
    for listener in &listeners {
        let ListenerTrigger::FileWatch(trigger) = &listener.trigger else {
            continue;
        };
        if !listener.should_arm() {
            arm_results.push((listener.id.clone(), false, None));
            continue;
        }
        match file::arm(app.clone(), listener, trigger) {
            Ok(watch) => {
                watchers.insert(listener.id.clone(), watch);
                arm_results.push((listener.id.clone(), true, None));
            }
            Err(error) => {
                crate::utils::logging::log_debug(&format!(
                    "[automation] listener {} could not arm: {error}",
                    listener.id
                ));
                arm_results.push((listener.id.clone(), false, Some(error)));
            }
        }
    }
    // Replacing the map drops the previous watchers, which stops their
    // debounce threads through the disconnected channel.
    if let Ok(mut slot) = supervisor.watchers.lock() {
        *slot = watchers;
    }

    // ---- webhook server ----
    // Arming domains are independent: a port conflict must not stop the file
    // and poll listeners from running.
    let wants_webhook = listeners.iter().any(|listener| {
        listener.should_arm() && matches!(listener.trigger, ListenerTrigger::Webhook(_))
    });
    let webhook_state = reconcile_webhook(app, wants_webhook).await;
    for listener in &listeners {
        if !matches!(listener.trigger, ListenerTrigger::Webhook(_)) {
            continue;
        }
        if !listener.should_arm() {
            arm_results.push((listener.id.clone(), false, None));
            continue;
        }
        match &webhook_state {
            Ok(()) => arm_results.push((listener.id.clone(), true, None)),
            Err(error) => arm_results.push((listener.id.clone(), false, Some(error.clone()))),
        }
    }

    // ---- poll listeners have nothing to bind ----
    for listener in &listeners {
        if matches!(listener.trigger, ListenerTrigger::WebPoll(_)) {
            arm_results.push((listener.id.clone(), listener.should_arm(), None));
        }
    }

    persist_arming(&arm_results);
    prune_orphan_secrets(&listeners);
    launch::emit_listeners_updated(app);
}

async fn reconcile_webhook(app: &AppHandle, wanted: bool) -> Result<(), String> {
    let supervisor = app.state::<ListenerSupervisor>();
    let mut server = supervisor.webhook.lock().await;
    match (wanted, server.is_some()) {
        (true, true) | (false, false) => Ok(()),
        (false, true) => {
            if let Some(running) = server.take() {
                running.handle.abort();
            }
            Ok(())
        }
        (true, false) => match webhook::serve(app.clone()).await {
            Ok((handle, address)) => {
                crate::utils::logging::log_debug(&format!(
                    "[automation] webhook gateway listening on {address}"
                ));
                *server = Some(WebhookServer { handle });
                Ok(())
            }
            Err(error) => {
                crate::utils::logging::log_debug(&format!(
                    "[automation] webhook gateway unavailable: {error}"
                ));
                Err(error)
            }
        },
    }
}

/// Write arming outcomes back without disturbing configuration.
fn persist_arming(results: &[(String, bool, Option<String>)]) {
    if results.is_empty() {
        return;
    }
    let outcome = listeners::mutate_listeners(|stored| {
        for (id, armed, error) in results {
            if let Some(listener) = stored.iter_mut().find(|listener| &listener.id == id) {
                listener.runtime.armed = *armed;
                listener.runtime.arm_error = error.clone();
            }
        }
        Ok(())
    });
    if let Err(error) = outcome {
        crate::utils::logging::log_debug(&format!(
            "[automation] could not persist listener arming state: {error}"
        ));
    }
}

/// Drop credentials belonging to listeners that no longer exist, so removing a
/// webhook does not leave a live secret behind.
fn prune_orphan_secrets(current: &[AutomationListener]) {
    let ids: Vec<String> = current.iter().map(|listener| listener.id.clone()).collect();
    if let Err(error) = listeners::secrets::prune_secrets(&ids) {
        crate::utils::logging::log_debug(&format!(
            "[automation] could not prune listener secrets: {error}"
        ));
    }
}

/// Stop the webhook server and re-arm from scratch.
///
/// Needed because the gateway's bind settings live outside the listener config
/// and therefore do not move the arming fingerprint; clearing the fingerprint
/// is what makes the next reconcile actually rebind.
pub async fn restart_webhook_gateway(app: &AppHandle) {
    {
        let supervisor = app.state::<ListenerSupervisor>();
        if let Some(running) = supervisor.webhook.lock().await.take() {
            running.handle.abort();
        }
        *supervisor.fingerprint.lock().await = None;
    }
    reconcile(app).await;
}

/// Start the supervisor loop, replacing any previous one.
pub async fn start(app: AppHandle) {
    if app.try_state::<ListenerSupervisor>().is_none() {
        app.manage(ListenerSupervisor::default());
    }
    {
        let supervisor = app.state::<ListenerSupervisor>();
        let mut tick = supervisor.tick.lock().await;
        if let Some(existing) = tick.take() {
            existing.abort();
        }
    }

    reconcile(&app).await;

    let app_for_loop = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
            let state = app_for_loop.state::<crate::state::AppState>();
            // Listeners honor the same global pause as schedules; an operator
            // pausing unattended automation means all of it.
            if state.automation_schedules_paused.load(Ordering::SeqCst) {
                continue;
            }
            reconcile(&app_for_loop).await;
            poll::tick(&app_for_loop).await;
        }
    });

    let supervisor = app.state::<ListenerSupervisor>();
    let mut tick = supervisor.tick.lock().await;
    *tick = Some(handle);
}

#[cfg(test)]
mod tests {
    use wardian_core::listeners::{
        arming_fingerprint, AutomationListener, FileWatchTrigger, ListenerRuntime, ListenerTrigger,
        DEFAULT_DEBOUNCE_MS,
    };

    fn listener(id: &str, enabled: bool) -> AutomationListener {
        AutomationListener {
            id: id.into(),
            blueprint_id: "audit".into(),
            name: "Audit".into(),
            enabled,
            trigger: ListenerTrigger::FileWatch(FileWatchTrigger {
                path: "/watched".into(),
                recursive: true,
                patterns: Vec::new(),
                ignore: Vec::new(),
                events: Vec::new(),
                debounce_ms: DEFAULT_DEBOUNCE_MS,
            }),
            provider: None,
            workspace: None,
            input: serde_json::json!({}),
            bindings: Default::default(),
            assignments: Default::default(),
            overlap: None,
            runtime: ListenerRuntime::default(),
        }
    }

    #[test]
    fn arming_state_write_back_does_not_itself_request_a_rearm() {
        let mut listeners = vec![listener("a", true)];
        let before = arming_fingerprint(&listeners);

        // Exactly what `persist_arming` writes after a successful reconcile.
        listeners[0].runtime.armed = true;
        listeners[0].runtime.arm_error = None;

        assert_eq!(
            before,
            arming_fingerprint(&listeners),
            "persisting arming results must not trigger another reconcile"
        );
    }

    #[test]
    fn disabling_a_listener_requests_a_rearm() {
        let armed = vec![listener("a", true)];
        let disabled = vec![listener("a", false)];
        assert_ne!(arming_fingerprint(&armed), arming_fingerprint(&disabled));
    }

    #[test]
    fn an_auto_disabled_listener_requests_a_rearm_without_the_enabled_flag_moving() {
        let mut listeners = vec![listener("a", true)];
        let before = arming_fingerprint(&listeners);
        listeners[0].runtime.disabled_reason = Some("rate ceiling".into());
        assert_ne!(before, arming_fingerprint(&listeners));
        assert!(listeners[0].enabled);
    }
}

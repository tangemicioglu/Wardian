//! Browser surface commands, shared by the workbench and the control plane.
//!
//! Every operation resolves a session through the broker, so `wardian browser`
//! and the surface act on exactly the same runtime with the same rules.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::browser_session::{
    discover_engine, BrowserError, BrowserSession, BrowserSessionBroker, ElementAction, LoadState,
    OpenBrowserRequest, PageField, PointerEvent, ScreencastAttachment, Viewport, WaitCondition,
};
use crate::state::AppState;
use wardian_core::browser::{
    BrowserActionResult, BrowserCookie, BrowserGetResult, BrowserScreenshotResult,
    BrowserSessionSummary, ConsoleEntry, CookieAction, DownloadRecord, NetworkAction,
    NetworkOutcome, PageSnapshot, StorageAction, StorageArea, StorageOutcome,
};

/// Event the frontend listens for to open a surface for a new session.
pub const BROWSER_SURFACE_OPEN_EVENT: &str = "browser-surface-open";
/// Event carrying session lifecycle and frame updates to the frontend.
pub const BROWSER_SESSION_EVENT: &str = "browser-session-event";
/// Default `wait` budget when a caller does not supply one.
pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 15_000;

/// Whether this host can back a browser surface at all.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BrowserEngineStatus {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Present when no engine is available; names the fix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Republishes broker events to the frontend for the lifetime of the app.
pub fn start_browser_session_event_bridge(app: AppHandle, broker: Arc<BrowserSessionBroker>) {
    let mut events = broker.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let _ = app.emit(BROWSER_SESSION_EVENT, event);
                }
                // Lagging drops screencast frames, which the next frame
                // repairs; closing means the app is going away.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Reports engine availability without launching anything.
pub fn engine_status() -> BrowserEngineStatus {
    match discover_engine() {
        Ok(binary) => BrowserEngineStatus {
            available: true,
            engine: Some(binary.kind.as_str().to_string()),
            path: Some(binary.path.display().to_string()),
            detail: None,
        },
        Err(error) => BrowserEngineStatus {
            available: false,
            engine: None,
            path: None,
            detail: Some(error.to_string()),
        },
    }
}

/// Builds a wait condition from the mutually exclusive CLI flags.
///
/// Exactly one predicate may be given; supplying several would silently honor
/// only the first, so it is refused instead.
pub fn wait_condition_from_parts(
    load_state: Option<&str>,
    selector: Option<&str>,
    text: Option<&str>,
    url_contains: Option<&str>,
    function: Option<&str>,
) -> Result<WaitCondition, BrowserError> {
    let mut chosen: Vec<WaitCondition> = Vec::new();
    if let Some(load_state) = load_state {
        let parsed = LoadState::parse(load_state).ok_or_else(|| BrowserError::Invalid {
            detail: format!(
                "{load_state} is not a load state; use idle, loading, or complete"
            ),
        })?;
        chosen.push(WaitCondition::LoadState(parsed));
    }
    if let Some(selector) = selector {
        chosen.push(WaitCondition::Selector(selector.to_string()));
    }
    if let Some(text) = text {
        chosen.push(WaitCondition::Text(text.to_string()));
    }
    if let Some(fragment) = url_contains {
        chosen.push(WaitCondition::UrlContains(fragment.to_string()));
    }
    if let Some(function) = function {
        chosen.push(WaitCondition::Function(function.to_string()));
    }
    match chosen.len() {
        0 => Err(BrowserError::Invalid {
            detail: "wait needs one of --load-state, --selector, --text, --url-contains, or --function"
                .to_string(),
        }),
        1 => Ok(chosen.into_iter().next().expect("one condition")),
        _ => Err(BrowserError::Invalid {
            detail: "wait accepts only one condition at a time".to_string(),
        }),
    }
}

/// Builds a DOM action from the CLI verb and its optional argument.
pub fn element_action_from_parts(
    action: &str,
    value: Option<&str>,
) -> Result<ElementAction, BrowserError> {
    let required = |verb: &str| -> Result<String, BrowserError> {
        value
            .map(str::to_string)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| BrowserError::Invalid {
                detail: format!("{verb} needs a value"),
            })
    };
    match action {
        "click" => Ok(ElementAction::Click),
        "hover" => Ok(ElementAction::Hover),
        "scroll" => Ok(ElementAction::Scroll),
        "fill" => Ok(ElementAction::Fill(required("fill")?)),
        "press" => Ok(ElementAction::Press(required("press")?)),
        "select" => Ok(ElementAction::Select(required("select")?)),
        other => Err(BrowserError::Invalid {
            detail: format!(
                "{other} is not a browser action; use click, fill, press, select, hover, or scroll"
            ),
        }),
    }
}

fn broker(app: &AppHandle) -> Arc<BrowserSessionBroker> {
    Arc::clone(&app.state::<AppState>().browser_sessions)
}

async fn resolve(app: &AppHandle, target: &str) -> Result<Arc<BrowserSession>, BrowserError> {
    broker(app).resolve(target).await
}

/// Opens a session and, unless detached, asks the frontend to surface it.
pub async fn open_session(
    app: &AppHandle,
    url: Option<String>,
    agent: Option<String>,
    workspace: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    detached: bool,
) -> Result<BrowserSessionSummary, BrowserError> {
    let viewport = match (width, height) {
        (Some(width), Some(height)) => Some(Viewport { width, height }),
        (None, None) => None,
        _ => {
            return Err(BrowserError::Invalid {
                detail: "a viewport needs both a width and a height".to_string(),
            })
        }
    };
    let session = broker(app)
        .open(OpenBrowserRequest {
            url,
            owner_agent_id: agent,
            workspace,
            viewport,
        })
        .await?;
    let summary = session.summary().await;
    if !detached {
        // Queue first, then emit. A listener that is already installed opens
        // the surface immediately and the drained duplicate focuses it rather
        // than opening a second one, because the surface is `focus_resource`.
        broker(app).queue_surface_open(summary.clone()).await;
        let _ = app.emit(BROWSER_SURFACE_OPEN_EVENT, &summary);
    }
    Ok(summary)
}

pub async fn list_sessions(app: &AppHandle) -> Vec<BrowserSessionSummary> {
    broker(app).list().await
}

pub async fn close_session(app: &AppHandle, target: &str) -> Result<String, BrowserError> {
    broker(app).close(target).await
}

/// Applies `back`, `forward`, `reload`, `stop`, or a URL.
///
/// `lease_token` is `None` on the control-plane path and `Some` for anything a
/// surface initiates, so a mirroring pane cannot navigate the shared page.
pub async fn navigate_session(
    app: &AppHandle,
    target: &str,
    action: &str,
    lease_token: Option<&str>,
) -> Result<BrowserSessionSummary, BrowserError> {
    let session = resolve(app, target).await?;
    session.require_drive(lease_token).await?;
    match action {
        "back" => session.traverse_history(-1).await?,
        "forward" => session.traverse_history(1).await?,
        "reload" => session.reload().await?,
        "stop" => session.stop_loading().await?,
        url => session.navigate(url).await?,
    }
    Ok(session.summary().await)
}

pub async fn get_field(
    app: &AppHandle,
    target: &str,
    field: &str,
    selector: Option<&str>,
) -> Result<BrowserGetResult, BrowserError> {
    let parsed = PageField::parse(field).ok_or_else(|| BrowserError::Invalid {
        detail: format!("{field} is not readable; use url, title, text, or html"),
    })?;
    let session = resolve(app, target).await?;
    let value = session.get(parsed, selector).await?;
    Ok(BrowserGetResult {
        browser_id: session.browser_id().to_string(),
        field: field.to_string(),
        value,
    })
}

pub async fn wait_for(
    app: &AppHandle,
    target: &str,
    condition: &WaitCondition,
    timeout_ms: Option<u64>,
) -> Result<BrowserSessionSummary, BrowserError> {
    let session = resolve(app, target).await?;
    session
        .wait(condition, timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS))
        .await?;
    Ok(session.summary().await)
}

pub async fn snapshot_session(
    app: &AppHandle,
    target: &str,
    interactive: bool,
) -> Result<PageSnapshot, BrowserError> {
    resolve(app, target).await?.snapshot(interactive).await
}

/// Performs an action, optionally folding the follow-up snapshot into the result.
pub async fn act_on_session(
    app: &AppHandle,
    target: &str,
    element_ref: &str,
    action: &ElementAction,
    snapshot_after: bool,
) -> Result<BrowserActionResult, BrowserError> {
    let session = resolve(app, target).await?;
    session.act(element_ref, action).await?;
    // A click can navigate. Re-snapshotting is best effort: the action itself
    // already succeeded, and reporting it as failed would be wrong.
    let snapshot = if snapshot_after {
        session.snapshot(true).await.ok()
    } else {
        None
    };
    Ok(BrowserActionResult {
        browser_id: session.browser_id().to_string(),
        action: action.name().to_string(),
        element_ref: element_ref.to_string(),
        snapshot,
    })
}

pub async fn screenshot_session(
    app: &AppHandle,
    target: &str,
    path: &str,
    full_page: bool,
) -> Result<BrowserScreenshotResult, BrowserError> {
    let session = resolve(app, target).await?;
    let resolved = PathBuf::from(path);
    if resolved.as_os_str().is_empty() {
        return Err(BrowserError::Invalid {
            detail: "a screenshot needs an output path".to_string(),
        });
    }
    session.screenshot(&resolved, full_page).await?;
    Ok(BrowserScreenshotResult {
        browser_id: session.browser_id().to_string(),
        path: resolved.display().to_string(),
        full_page,
    })
}

pub async fn set_session_viewport(
    app: &AppHandle,
    target: &str,
    width: Option<u32>,
    height: Option<u32>,
    reset: bool,
    lease_token: Option<&str>,
) -> Result<BrowserSessionSummary, BrowserError> {
    let session = resolve(app, target).await?;
    session.require_drive(lease_token).await?;
    let viewport = if reset {
        None
    } else {
        match (width, height) {
            (Some(width), Some(height)) => Some(Viewport { width, height }),
            _ => {
                return Err(BrowserError::Invalid {
                    detail: "viewport needs both a width and a height, or --reset".to_string(),
                })
            }
        }
    };
    session.set_viewport(viewport).await?;
    Ok(session.summary().await)
}

pub async fn eval_in_session(
    app: &AppHandle,
    target: &str,
    expression: &str,
) -> Result<Value, BrowserError> {
    resolve(app, target).await?.eval(expression).await
}

pub async fn console_for_session(
    app: &AppHandle,
    target: &str,
    level: Option<&str>,
    clear: bool,
) -> Result<Vec<ConsoleEntry>, BrowserError> {
    let level = match level {
        Some(level) => Some(normalize_console_filter(level)?),
        None => None,
    };
    Ok(resolve(app, target).await?.console(level, clear).await)
}

/// Validates a `--level` value against the three severities capture collapses to.
pub fn normalize_console_filter(level: &str) -> Result<&'static str, BrowserError> {
    match level.to_ascii_lowercase().as_str() {
        "error" => Ok("error"),
        "warn" | "warning" => Ok("warning"),
        "info" | "log" => Ok("info"),
        other => Err(BrowserError::Invalid {
            detail: format!("{other} is not a console level; use error, warning, or info"),
        }),
    }
}

/// Runs one `network` verb and returns whichever shape it produces.
pub async fn network_for_session(
    app: &AppHandle,
    target: &str,
    action: &NetworkAction,
) -> Result<NetworkOutcome, BrowserError> {
    let session = resolve(app, target).await?;
    match action {
        NetworkAction::List { filter } => Ok(NetworkOutcome::List {
            entries: session.network(filter).await,
        }),
        NetworkAction::Detail { request_id, body } => Ok(NetworkOutcome::Detail {
            detail: Box::new(session.network_detail(request_id, *body).await?),
        }),
        NetworkAction::Clear => {
            session.clear_network().await;
            Ok(NetworkOutcome::Cleared)
        }
    }
}

/// Runs one `cookies` verb. Only `list` returns cookies; the rest return none.
pub async fn cookies_for_session(
    app: &AppHandle,
    target: &str,
    action: &CookieAction,
) -> Result<Vec<BrowserCookie>, BrowserError> {
    resolve(app, target).await?.cookies(action).await
}

/// Runs one `storage` verb against one area.
pub async fn storage_for_session(
    app: &AppHandle,
    target: &str,
    area: StorageArea,
    action: &StorageAction,
) -> Result<StorageOutcome, BrowserError> {
    let session = resolve(app, target).await?;
    match action {
        StorageAction::Get { key: Some(key) } => Ok(StorageOutcome::Value {
            value: session.storage_get(area, key).await?,
        }),
        StorageAction::Get { key: None } => Ok(StorageOutcome::Snapshot {
            snapshot: session.storage(area).await?,
        }),
        mutation => {
            session.storage_mutate(area, mutation).await?;
            Ok(StorageOutcome::Applied)
        }
    }
}

pub async fn downloads_for_session(
    app: &AppHandle,
    target: &str,
    clear: bool,
) -> Result<Vec<DownloadRecord>, BrowserError> {
    let session = resolve(app, target).await?;
    let records = session.downloads().await;
    if clear {
        session.clear_downloads().await;
    }
    Ok(records)
}

// ---------------------------------------------------------------------------
// Tauri commands used by the workbench surface
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct BrowserPointerRequest {
    pub browser_id: String,
    /// Lease token from `attach_browser_screencast`. Required: an absent one
    /// would otherwise be the control-plane path and bypass the lease.
    pub lease_token: String,
    pub event_type: String,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub button: Option<String>,
    #[serde(default)]
    pub click_count: Option<u32>,
    #[serde(default)]
    pub modifiers: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrowserWheelRequest {
    pub browser_id: String,
    pub lease_token: String,
    pub x: f64,
    pub y: f64,
    pub delta_x: f64,
    pub delta_y: f64,
    #[serde(default)]
    pub modifiers: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrowserKeyRequest {
    pub browser_id: String,
    pub lease_token: String,
    pub event_type: String,
    pub key: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub modifiers: Option<u32>,
}

fn command_error(error: BrowserError) -> String {
    error.to_string()
}

#[tauri::command]
pub fn browser_engine_status() -> BrowserEngineStatus {
    engine_status()
}

#[tauri::command]
pub async fn open_browser_session(
    url: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    app: AppHandle,
) -> Result<BrowserSessionSummary, String> {
    // The surface opens its own presentation, so the frontend must not be told
    // to open a second one.
    open_session(&app, url, None, None, width, height, true)
        .await
        .map_err(command_error)
}

/// Surface opens still waiting to be acknowledged.
#[tauri::command]
pub async fn pending_browser_surface_opens(
    state: State<'_, AppState>,
) -> Result<Vec<BrowserSessionSummary>, String> {
    Ok(state.browser_sessions.pending_surface_opens().await)
}

/// Confirms one open was surfaced, so no later reader repeats it.
#[tauri::command]
pub async fn ack_browser_surface_open(
    browser_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.browser_sessions.ack_surface_open(&browser_id).await;
    Ok(())
}

#[tauri::command]
pub async fn list_browser_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<BrowserSessionSummary>, String> {
    Ok(state.browser_sessions.list().await)
}

#[tauri::command]
pub async fn get_browser_session(
    browser_id: String,
    state: State<'_, AppState>,
) -> Result<Option<BrowserSessionSummary>, String> {
    match state.browser_sessions.resolve(&browser_id).await {
        Ok(session) => Ok(Some(session.summary().await)),
        Err(BrowserError::NotFound { .. }) => Ok(None),
        Err(error) => Err(command_error(error)),
    }
}

#[tauri::command]
pub async fn close_browser_session(
    browser_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .browser_sessions
        .close(&browser_id)
        .await
        .map(|_| ())
        .map_err(command_error)
}

#[tauri::command]
pub async fn navigate_browser_session(
    browser_id: String,
    action: String,
    lease_token: String,
    app: AppHandle,
) -> Result<BrowserSessionSummary, String> {
    navigate_session(&app, &browser_id, &action, Some(&lease_token))
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn attach_browser_screencast(
    browser_id: String,
    presentation_id: String,
    state: State<'_, AppState>,
) -> Result<ScreencastAttachment, String> {
    state
        .browser_sessions
        .resolve(&browser_id)
        .await
        .map_err(command_error)?
        .attach_screencast(&presentation_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn detach_browser_screencast(
    browser_id: String,
    lease_token: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .browser_sessions
        .resolve(&browser_id)
        .await
        .map_err(command_error)?
        .detach_screencast(&lease_token)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn send_browser_pointer(
    request: BrowserPointerRequest,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .browser_sessions
        .resolve(&request.browser_id)
        .await
        .map_err(command_error)?
        .dispatch_mouse(
            Some(request.lease_token.as_str()),
            &PointerEvent {
                event_type: &request.event_type,
                x: request.x,
                y: request.y,
                button: request.button.as_deref().unwrap_or("left"),
                click_count: request.click_count.unwrap_or(1),
                modifiers: request.modifiers.unwrap_or(0),
            },
        )
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn send_browser_wheel(
    request: BrowserWheelRequest,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .browser_sessions
        .resolve(&request.browser_id)
        .await
        .map_err(command_error)?
        .dispatch_wheel(
            Some(request.lease_token.as_str()),
            request.x,
            request.y,
            request.delta_x,
            request.delta_y,
            request.modifiers.unwrap_or(0),
        )
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn send_browser_key(
    request: BrowserKeyRequest,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let session = state
        .browser_sessions
        .resolve(&request.browser_id)
        .await
        .map_err(command_error)?;
    session
        .dispatch_key(
            Some(request.lease_token.as_str()),
            &request.event_type,
            &request.key,
            request.code.as_deref().unwrap_or(""),
            request.text.as_deref(),
            request.modifiers.unwrap_or(0),
        )
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn set_browser_viewport(
    browser_id: String,
    width: u32,
    height: u32,
    lease_token: String,
    app: AppHandle,
) -> Result<BrowserSessionSummary, String> {
    set_session_viewport(
        &app,
        &browser_id,
        Some(width),
        Some(height),
        false,
        Some(&lease_token),
    )
    .await
    .map_err(command_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_wait_flag_produces_its_condition() {
        assert_eq!(
            wait_condition_from_parts(Some("complete"), None, None, None, None).expect("condition"),
            WaitCondition::LoadState(LoadState::Complete)
        );
        assert_eq!(
            wait_condition_from_parts(None, Some("#ready"), None, None, None).expect("condition"),
            WaitCondition::Selector("#ready".to_string())
        );
        assert_eq!(
            wait_condition_from_parts(None, None, None, Some("/dashboard"), None)
                .expect("condition"),
            WaitCondition::UrlContains("/dashboard".to_string())
        );
    }

    #[test]
    fn no_wait_flag_names_every_option() {
        let error = wait_condition_from_parts(None, None, None, None, None).expect_err("none");
        assert!(error.to_string().contains("--load-state"));
        assert!(error.to_string().contains("--function"));
    }

    #[test]
    fn several_wait_flags_are_refused_rather_than_silently_narrowed() {
        let error = wait_condition_from_parts(None, Some("#a"), Some("hello"), None, None)
            .expect_err("ambiguous");
        assert!(error.to_string().contains("only one condition"));
    }

    #[test]
    fn an_unknown_load_state_lists_the_valid_ones() {
        let error =
            wait_condition_from_parts(Some("settled"), None, None, None, None).expect_err("bad");
        assert!(error.to_string().contains("idle, loading, or complete"));
    }

    #[test]
    fn actions_without_arguments_parse_from_their_verb() {
        assert_eq!(
            element_action_from_parts("click", None).expect("click"),
            ElementAction::Click
        );
        assert_eq!(
            element_action_from_parts("hover", None).expect("hover"),
            ElementAction::Hover
        );
        assert_eq!(
            element_action_from_parts("scroll", None).expect("scroll"),
            ElementAction::Scroll
        );
    }

    #[test]
    fn actions_that_need_a_value_refuse_an_empty_one() {
        for verb in ["fill", "press", "select"] {
            assert!(
                element_action_from_parts(verb, None).is_err(),
                "{verb} must require a value"
            );
            assert!(
                element_action_from_parts(verb, Some("")).is_err(),
                "{verb} must reject an empty value"
            );
        }
        assert_eq!(
            element_action_from_parts("fill", Some("hello")).expect("fill"),
            ElementAction::Fill("hello".to_string())
        );
    }

    #[test]
    fn an_unknown_action_lists_the_supported_verbs() {
        let error = element_action_from_parts("teleport", None).expect_err("unknown");
        assert!(error.to_string().contains("click, fill, press, select, hover, or scroll"));
    }

    #[test]
    fn engine_status_reports_a_detail_when_no_engine_exists() {
        let status = engine_status();
        if status.available {
            assert!(status.path.is_some());
            assert!(status.detail.is_none());
        } else {
            assert!(
                status.detail.expect("detail").contains("WARDIAN_BROWSER_BINARY"),
                "an unavailable engine must name the override"
            );
        }
    }
}

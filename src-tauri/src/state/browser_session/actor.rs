//! Browser session lifecycle and the operations surfaces and agents perform.
//!
//! A browser session is a backend-owned runtime resource, like a PTY session.
//! Workbench surfaces attach to it as presentations; detaching a presentation —
//! closing a tab, unmounting the renderer — never disturbs the runtime. A
//! session ends only on explicit close, on its owning agent's termination, or
//! on app exit.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
pub use wardian_core::browser::{
    BrowserSessionSummary, ConsoleEntry, LoadState, Viewport, DEFAULT_VIEWPORT_HEIGHT,
    DEFAULT_VIEWPORT_WIDTH,
};
use serde_json::{json, Value};
use tokio::process::Child;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use super::cdp::{required_str, CdpConnection, CdpError, CdpEvent, DISCONNECTED_METHOD};
use super::engine::{discover_engine, launch_engine, EngineBinary, EngineError, EngineKind};
use super::snapshot::{
    action_expression, parse_snapshot, snapshot_expression, PageSnapshot, RefError,
    SnapshotLedger,
};

/// How often a `wait` predicate is re-evaluated.
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Console entries retained per session for the surface's error badge.
const CONSOLE_BUFFER: usize = 200;
/// Buffered session events. Screencast frames dominate this channel.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// What a session tells the rest of the app about itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserSessionEvent {
    /// A screencast frame, base64-encoded JPEG.
    Frame {
        browser_id: String,
        data: String,
        width: u32,
        height: u32,
    },
    /// Navigation, title, or load-state change.
    State {
        browser_id: String,
        summary: BrowserSessionSummary,
    },
    /// A console message the page produced.
    Console {
        browser_id: String,
        entry: ConsoleEntry,
    },
    /// The session's runtime is gone. Surfaces should show the reopen path.
    Closed { browser_id: String, reason: String },
}

impl BrowserSessionEvent {
    pub fn browser_id(&self) -> &str {
        match self {
            BrowserSessionEvent::Frame { browser_id, .. }
            | BrowserSessionEvent::State { browser_id, .. }
            | BrowserSessionEvent::Console { browser_id, .. }
            | BrowserSessionEvent::Closed { browser_id, .. } => browser_id,
        }
    }
}

/// Why a browser operation failed.
#[derive(Debug)]
pub enum BrowserError {
    /// No session matches the given target.
    NotFound { target: String },
    /// The target matched more than one session.
    Ambiguous { target: String, matches: Vec<String> },
    /// The host has no usable browser, or one could not be started.
    Engine(EngineError),
    /// The protocol call failed.
    Cdp(CdpError),
    /// A snapshot ref could not be used.
    Ref(RefError),
    /// A `wait` predicate never became true.
    WaitTimeout { condition: String, timeout_ms: u64 },
    /// The caller supplied something this operation cannot accept.
    Invalid { detail: String },
    /// A filesystem operation around a screenshot failed.
    Io { detail: String },
    /// A mirrored presentation tried to drive the page.
    ReadOnlyPresentation,
}

impl std::fmt::Display for BrowserError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserError::NotFound { target } => write!(
                formatter,
                "no browser session matches {target}. Run `wardian browser list` to see open sessions."
            ),
            BrowserError::Ambiguous { target, matches } => write!(
                formatter,
                "{target} matches {} sessions ({}). Use a full id.",
                matches.len(),
                matches.join(", ")
            ),
            BrowserError::Engine(error) => write!(formatter, "{error}"),
            BrowserError::Cdp(error) => write!(formatter, "{error}"),
            BrowserError::Ref(error) => write!(formatter, "{error}"),
            BrowserError::WaitTimeout {
                condition,
                timeout_ms,
            } => write!(formatter, "timed out after {timeout_ms}ms waiting for {condition}"),
            BrowserError::Invalid { detail } => write!(formatter, "{detail}"),
            BrowserError::Io { detail } => write!(formatter, "{detail}"),
            BrowserError::ReadOnlyPresentation => write!(
                formatter,
                "this presentation is mirroring the page read-only; another surface holds the drive lease"
            ),
        }
    }
}

impl std::error::Error for BrowserError {}

impl BrowserError {
    /// Stable machine-readable code for `--json` consumers.
    pub fn code(&self) -> &'static str {
        match self {
            BrowserError::NotFound { .. } => "browser_not_found",
            BrowserError::Ambiguous { .. } => "browser_ambiguous",
            BrowserError::Engine(_) => "browser_engine_unavailable",
            BrowserError::Cdp(_) => "browser_protocol_error",
            BrowserError::Ref(error) => error.code(),
            BrowserError::WaitTimeout { .. } => "browser_wait_timeout",
            BrowserError::Invalid { .. } => "browser_invalid_request",
            BrowserError::Io { .. } => "browser_io_error",
            BrowserError::ReadOnlyPresentation => "browser_read_only_presentation",
        }
    }
}

impl From<CdpError> for BrowserError {
    fn from(error: CdpError) -> Self {
        BrowserError::Cdp(error)
    }
}

impl From<RefError> for BrowserError {
    fn from(error: RefError) -> Self {
        BrowserError::Ref(error)
    }
}

/// A condition `wait` can block on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitCondition {
    LoadState(LoadState),
    Selector(String),
    Text(String),
    UrlContains(String),
    Function(String),
}

impl WaitCondition {
    fn describe(&self) -> String {
        match self {
            WaitCondition::LoadState(state) => format!("load state {}", state.as_str()),
            WaitCondition::Selector(selector) => format!("selector {selector:?}"),
            WaitCondition::Text(text) => format!("text {text:?}"),
            WaitCondition::UrlContains(fragment) => format!("url containing {fragment:?}"),
            WaitCondition::Function(expression) => format!("expression {expression:?}"),
        }
    }

    /// The JavaScript predicate this condition polls, when it needs one.
    fn predicate(&self) -> Option<String> {
        match self {
            WaitCondition::LoadState(LoadState::Complete) => {
                Some("document.readyState === 'complete'".to_string())
            }
            WaitCondition::LoadState(LoadState::Loading) => {
                Some("document.readyState === 'loading'".to_string())
            }
            WaitCondition::LoadState(LoadState::Idle) => {
                Some("document.readyState !== 'loading'".to_string())
            }
            WaitCondition::LoadState(LoadState::Failed) => None,
            WaitCondition::Selector(selector) => Some(format!(
                "document.querySelector({}) !== null",
                json!(selector)
            )),
            WaitCondition::Text(text) => Some(format!(
                "(document.body ? document.body.innerText : '').includes({})",
                json!(text)
            )),
            WaitCondition::UrlContains(fragment) => Some(format!(
                "window.location.href.includes({})",
                json!(fragment)
            )),
            WaitCondition::Function(expression) => Some(format!("!!({expression})")),
        }
    }
}

/// One pointer event forwarded from a surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent<'a> {
    pub event_type: &'a str,
    pub x: f64,
    pub y: f64,
    pub button: &'a str,
    pub click_count: u32,
    pub modifiers: u32,
}

/// A DOM action against a snapshot ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementAction {
    Click,
    Hover,
    Fill(String),
    Press(String),
    Select(String),
    Scroll,
}

impl ElementAction {
    /// The verb an operator typed, echoed back in results and logs.
    pub fn name(&self) -> &'static str {
        match self {
            ElementAction::Click => "click",
            ElementAction::Hover => "hover",
            ElementAction::Fill(_) => "fill",
            ElementAction::Press(_) => "press",
            ElementAction::Select(_) => "select",
            ElementAction::Scroll => "scroll",
        }
    }
}

/// What `get` can read off the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageField {
    Url,
    Title,
    Text,
    Html,
}

impl PageField {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "url" => Some(PageField::Url),
            "title" => Some(PageField::Title),
            "text" => Some(PageField::Text),
            "html" => Some(PageField::Html),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct SessionState {
    url: String,
    title: String,
    load_state: LoadState,
    viewport: Viewport,
    ledger: SnapshotLedger,
    console: VecDeque<ConsoleEntry>,
    console_error_count: usize,
    /// Presentations currently streaming, in attach order.
    screencast_viewers: Vec<String>,
    /// The presentation allowed to drive the page. First attach wins.
    owner_presentation_id: Option<String>,
}

/// One live browser, its protocol session, and everything derived from it.
#[derive(Debug)]
pub struct BrowserSession {
    browser_id: String,
    short_ref: u32,
    owner_agent_id: Option<String>,
    workspace: Option<String>,
    engine: EngineKind,
    connection: Arc<CdpConnection>,
    cdp_session_id: String,
    profile_dir: PathBuf,
    child: Mutex<Option<Child>>,
    state: RwLock<SessionState>,
}

impl BrowserSession {
    pub fn browser_id(&self) -> &str {
        &self.browser_id
    }

    pub fn short_ref(&self) -> String {
        format!("browser:{}", self.short_ref)
    }

    pub fn owner_agent_id(&self) -> Option<&str> {
        self.owner_agent_id.as_deref()
    }

    pub async fn summary(&self) -> BrowserSessionSummary {
        let state = self.state.read().await;
        BrowserSessionSummary {
            browser_id: self.browser_id.clone(),
            short_ref: self.short_ref(),
            url: state.url.clone(),
            title: state.title.clone(),
            load_state: state.load_state,
            viewport: state.viewport,
            engine: self.engine,
            owner_agent_id: self.owner_agent_id.clone(),
            workspace: self.workspace.clone(),
            console_error_count: state.console_error_count,
        }
    }

    async fn evaluate(&self, expression: &str) -> Result<Value, BrowserError> {
        let result = self
            .connection
            .call_session(
                &self.cdp_session_id,
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                    "userGesture": true,
                }),
            )
            .await?;
        if let Some(exception) = result.get("exceptionDetails") {
            let text = exception
                .get("exception")
                .and_then(|value| value.get("description"))
                .and_then(Value::as_str)
                .or_else(|| exception.get("text").and_then(Value::as_str))
                .unwrap_or("evaluation failed");
            return Err(BrowserError::Cdp(CdpError::Protocol {
                method: "Runtime.evaluate".to_string(),
                code: 0,
                message: text.to_string(),
            }));
        }
        Ok(result
            .get("result")
            .and_then(|value| value.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// Navigates and blocks until the load settles or the deadline passes.
    pub async fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        let url = normalize_url(url)?;
        {
            let mut state = self.state.write().await;
            state.load_state = LoadState::Loading;
        }
        let result = self
            .connection
            .call_session(&self.cdp_session_id, "Page.navigate", json!({ "url": url }))
            .await?;
        if let Some(error) = result.get("errorText").and_then(Value::as_str) {
            let mut state = self.state.write().await;
            state.load_state = LoadState::Failed;
            return Err(BrowserError::Cdp(CdpError::Protocol {
                method: "Page.navigate".to_string(),
                code: 0,
                message: error.to_string(),
            }));
        }
        // Report the page being loaded rather than the one being left. The
        // commit arrives later as `Page.frameNavigated` and replaces this with
        // the post-redirect URL; until then, echoing `about:blank` back to a
        // caller who just asked for a URL is worse than being slightly early.
        {
            let mut state = self.state.write().await;
            state.url = url;
        }
        Ok(())
    }

    /// Moves through history. `delta` is negative for back, positive forward.
    pub async fn traverse_history(&self, delta: i64) -> Result<(), BrowserError> {
        let history = self
            .connection
            .call_session(&self.cdp_session_id, "Page.getNavigationHistory", json!({}))
            .await?;
        let current = history
            .get("currentIndex")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let entries = history
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let target = current + delta;
        if target < 0 || target as usize >= entries.len() {
            return Err(BrowserError::Invalid {
                detail: if delta < 0 {
                    "there is no earlier page in this session's history".to_string()
                } else {
                    "there is no later page in this session's history".to_string()
                },
            });
        }
        let entry_id = entries[target as usize]
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| BrowserError::Invalid {
                detail: "the browser returned a history entry without an id".to_string(),
            })?;
        self.connection
            .call_session(
                &self.cdp_session_id,
                "Page.navigateToHistoryEntry",
                json!({ "entryId": entry_id }),
            )
            .await?;
        Ok(())
    }

    pub async fn reload(&self) -> Result<(), BrowserError> {
        self.connection
            .call_session(&self.cdp_session_id, "Page.reload", json!({}))
            .await?;
        Ok(())
    }

    pub async fn stop_loading(&self) -> Result<(), BrowserError> {
        self.connection
            .call_session(&self.cdp_session_id, "Page.stopLoading", json!({}))
            .await?;
        Ok(())
    }

    /// Reads one field off the page, optionally scoped to a CSS selector.
    pub async fn get(
        &self,
        field: PageField,
        selector: Option<&str>,
    ) -> Result<String, BrowserError> {
        let expression = match (field, selector) {
            (PageField::Url, _) => "window.location.href".to_string(),
            (PageField::Title, _) => "document.title".to_string(),
            (PageField::Text, None) => {
                "document.body ? document.body.innerText : ''".to_string()
            }
            (PageField::Html, None) => "document.documentElement.outerHTML".to_string(),
            (PageField::Text, Some(selector)) => format!(
                "(() => {{ const node = document.querySelector({0}); if (!node) throw new Error('no element matches ' + {0}); return node.innerText; }})()",
                json!(selector)
            ),
            (PageField::Html, Some(selector)) => format!(
                "(() => {{ const node = document.querySelector({0}); if (!node) throw new Error('no element matches ' + {0}); return node.outerHTML; }})()",
                json!(selector)
            ),
        };
        let value = self.evaluate(&expression).await?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    /// Polls a condition until it holds or the timeout elapses.
    pub async fn wait(
        &self,
        condition: &WaitCondition,
        timeout_ms: u64,
    ) -> Result<(), BrowserError> {
        if let WaitCondition::LoadState(LoadState::Failed) = condition {
            return Err(BrowserError::Invalid {
                detail: "cannot wait for the failed load state; it is reported, not awaited"
                    .to_string(),
            });
        }
        let Some(predicate) = condition.predicate() else {
            return Err(BrowserError::Invalid {
                detail: format!("{} cannot be waited on", condition.describe()),
            });
        };
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            // An expression that throws mid-navigation is a not-yet, not a
            // failure; only the deadline ends the wait unsuccessfully.
            if let Ok(Value::Bool(true)) = self.evaluate(&predicate).await {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(BrowserError::WaitTimeout {
                    condition: condition.describe(),
                    timeout_ms,
                });
            }
            sleep(WAIT_POLL_INTERVAL).await;
        }
    }

    /// Walks the page and mints a fresh set of refs.
    pub async fn snapshot(&self, interactive_only: bool) -> Result<PageSnapshot, BrowserError> {
        let generation = self.state.read().await.ledger.current_generation();
        let raw = self
            .evaluate(&snapshot_expression(generation, interactive_only))
            .await?;
        let snapshot = parse_snapshot(generation, interactive_only, &raw)
            .map_err(|detail| BrowserError::Invalid { detail })?;
        let mut state = self.state.write().await;
        // The page may have navigated while the walker ran. Recording against a
        // generation that has since moved would hand back refs that are already
        // stale, so the snapshot is discarded rather than published.
        if state.ledger.current_generation() != generation {
            return Err(BrowserError::Ref(RefError::Stale {
                element_ref: "snapshot".to_string(),
                snapshot_generation: generation,
                current_generation: state.ledger.current_generation(),
            }));
        }
        state.ledger.record_snapshot(&snapshot.elements);
        state.url = snapshot.url.clone();
        state.title = snapshot.title.clone();
        Ok(snapshot)
    }

    /// Performs an action against a ref minted by the current snapshot.
    ///
    /// Three checks stand between a ref and a click: the snapshot generation,
    /// that the ref resolves to exactly one element, and that the element is
    /// still what the snapshot described. Any of them failing is a refusal,
    /// never a best guess.
    pub async fn act(
        &self,
        element_ref: &str,
        action: &ElementAction,
    ) -> Result<(), BrowserError> {
        let (generation, expected) = {
            let state = self.state.read().await;
            let identity = state.ledger.validate(element_ref)?.clone();
            (state.ledger.current_generation(), identity)
        };
        let body = match action {
            ElementAction::Click => {
                "node.scrollIntoView({ block: 'center' }); node.click();".to_string()
            }
            ElementAction::Hover => "node.scrollIntoView({ block: 'center' }); node.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })); node.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));".to_string(),
            ElementAction::Fill(value) => format!(
                "node.focus(); node.value = {}; node.dispatchEvent(new Event('input', {{ bubbles: true }})); node.dispatchEvent(new Event('change', {{ bubbles: true }}));",
                json!(value)
            ),
            ElementAction::Press(key) => format!(
                "node.focus(); for (const type of ['keydown', 'keypress', 'keyup']) {{ node.dispatchEvent(new KeyboardEvent(type, {{ key: {0}, bubbles: true }})); }} if ({0} === 'Enter' && node.form) {{ node.form.requestSubmit ? node.form.requestSubmit() : node.form.submit(); }}",
                json!(key)
            ),
            ElementAction::Select(value) => format!(
                "node.value = {}; node.dispatchEvent(new Event('change', {{ bubbles: true }}));",
                json!(value)
            ),
            ElementAction::Scroll => "node.scrollIntoView({ block: 'center' });".to_string(),
        };
        let outcome = self
            .evaluate(&action_expression(element_ref, generation, &expected, &body))
            .await?;
        match outcome.as_str() {
            Some("ok") => Ok(()),
            Some("ambiguous") => Err(BrowserError::Ref(RefError::Ambiguous {
                element_ref: element_ref.to_string(),
            })),
            Some("changed") => Err(BrowserError::Ref(RefError::Changed {
                element_ref: element_ref.to_string(),
            })),
            _ => Err(BrowserError::Ref(RefError::Detached {
                element_ref: element_ref.to_string(),
            })),
        }
    }

    /// Scrolls the page itself rather than an element.
    pub async fn scroll_page(&self, delta_x: f64, delta_y: f64) -> Result<(), BrowserError> {
        self.evaluate(&format!(
            "window.scrollBy({{ left: {delta_x}, top: {delta_y}, behavior: 'instant' }})"
        ))
        .await?;
        Ok(())
    }

    /// Captures a PNG and writes it to `path`.
    pub async fn screenshot(&self, path: &PathBuf, full_page: bool) -> Result<(), BrowserError> {
        let result = self
            .connection
            .call_session(
                &self.cdp_session_id,
                "Page.captureScreenshot",
                json!({ "format": "png", "captureBeyondViewport": full_page }),
            )
            .await?;
        let encoded = required_str("Page.captureScreenshot", &result, "data")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| BrowserError::Invalid {
                detail: format!("the browser returned an unreadable screenshot: {error}"),
            })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| BrowserError::Io {
                detail: format!("could not create {}: {error}", parent.display()),
            })?;
        }
        std::fs::write(path, bytes).map_err(|error| BrowserError::Io {
            detail: format!("could not write {}: {error}", path.display()),
        })
    }

    /// Overrides the rendered viewport, or clears the override when `None`.
    pub async fn set_viewport(&self, viewport: Option<Viewport>) -> Result<(), BrowserError> {
        let resolved = viewport.unwrap_or_default();
        if resolved.width == 0 || resolved.height == 0 {
            return Err(BrowserError::Invalid {
                detail: "viewport width and height must both be greater than zero".to_string(),
            });
        }
        self.connection
            .call_session(
                &self.cdp_session_id,
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width": resolved.width,
                    "height": resolved.height,
                    "deviceScaleFactor": 1,
                    "mobile": false,
                }),
            )
            .await?;
        self.state.write().await.viewport = resolved;
        Ok(())
    }

    /// Evaluates an arbitrary expression and returns its JSON value.
    pub async fn eval(&self, expression: &str) -> Result<Value, BrowserError> {
        self.evaluate(expression).await
    }

    /// Returns the retained console entries, newest last.
    pub async fn console(&self) -> Vec<ConsoleEntry> {
        self.state.read().await.console.iter().cloned().collect()
    }

    /// Starts streaming frames while at least one surface is watching.
    ///
    /// The first presentation to attach becomes the driver; later ones mirror
    /// it read-only, matching how a terminal session treats its presentations.
    pub async fn attach_screencast(&self, presentation_id: &str) -> Result<(), BrowserError> {
        let should_start = {
            let mut state = self.state.write().await;
            if !state.screencast_viewers.iter().any(|id| id == presentation_id) {
                state.screencast_viewers.push(presentation_id.to_string());
            }
            if state.owner_presentation_id.is_none() {
                state.owner_presentation_id = Some(presentation_id.to_string());
            }
            state.screencast_viewers.len() == 1
        };
        if should_start {
            self.connection
                .call_session(
                    &self.cdp_session_id,
                    "Page.startScreencast",
                    json!({ "format": "jpeg", "quality": 70, "everyNthFrame": 1 }),
                )
                .await?;
        }
        Ok(())
    }

    /// Stops streaming once the last surface detaches. The page keeps running.
    ///
    /// When the driver leaves, the lease passes to the longest-attached
    /// remaining presentation rather than leaving the page undrivable.
    pub async fn detach_screencast(&self, presentation_id: &str) -> Result<(), BrowserError> {
        let should_stop = {
            let mut state = self.state.write().await;
            state.screencast_viewers.retain(|id| id != presentation_id);
            if state.owner_presentation_id.as_deref() == Some(presentation_id) {
                state.owner_presentation_id = state.screencast_viewers.first().cloned();
            }
            state.screencast_viewers.is_empty()
        };
        if should_stop {
            self.connection
                .call_session(&self.cdp_session_id, "Page.stopScreencast", json!({}))
                .await?;
        }
        Ok(())
    }

    /// Whether a presentation currently holds the drive lease.
    ///
    /// An unattached caller — the CLI, or a surface that has not started
    /// streaming — is not a competing driver and is allowed through.
    pub async fn presentation_may_drive(&self, presentation_id: Option<&str>) -> bool {
        let Some(presentation_id) = presentation_id else {
            return true;
        };
        let state = self.state.read().await;
        match state.owner_presentation_id.as_deref() {
            None => true,
            Some(owner) => owner == presentation_id,
        }
    }

    /// Refuses a mutation from a presentation that does not hold the lease.
    async fn require_drive(&self, presentation_id: Option<&str>) -> Result<(), BrowserError> {
        if self.presentation_may_drive(presentation_id).await {
            return Ok(());
        }
        Err(BrowserError::ReadOnlyPresentation)
    }

    /// Forwards a pointer event from a surface into the page.
    pub async fn dispatch_mouse(
        &self,
        presentation_id: Option<&str>,
        event: &PointerEvent<'_>,
    ) -> Result<(), BrowserError> {
        self.require_drive(presentation_id).await?;
        if !matches!(
            event.event_type,
            "mousePressed" | "mouseReleased" | "mouseMoved" | "mouseWheel"
        ) {
            return Err(BrowserError::Invalid {
                detail: format!(
                    "{} is not a pointer event this surface forwards",
                    event.event_type
                ),
            });
        }
        self.connection
            .call_session(
                &self.cdp_session_id,
                "Input.dispatchMouseEvent",
                json!({
                    "type": event.event_type,
                    "x": event.x,
                    "y": event.y,
                    "button": event.button,
                    "clickCount": event.click_count,
                    "modifiers": event.modifiers,
                }),
            )
            .await?;
        Ok(())
    }

    /// Forwards a wheel event from a surface into the page.
    pub async fn dispatch_wheel(
        &self,
        presentation_id: Option<&str>,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        modifiers: u32,
    ) -> Result<(), BrowserError> {
        self.require_drive(presentation_id).await?;
        self.connection
            .call_session(
                &self.cdp_session_id,
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mouseWheel",
                    "x": x,
                    "y": y,
                    "deltaX": delta_x,
                    "deltaY": delta_y,
                    "modifiers": modifiers,
                }),
            )
            .await?;
        Ok(())
    }

    /// Forwards a key event from a surface into the page.
    pub async fn dispatch_key(
        &self,
        presentation_id: Option<&str>,
        event_type: &str,
        key: &str,
        code: &str,
        text: Option<&str>,
        modifiers: u32,
    ) -> Result<(), BrowserError> {
        self.require_drive(presentation_id).await?;
        if !matches!(event_type, "keyDown" | "keyUp" | "rawKeyDown" | "char") {
            return Err(BrowserError::Invalid {
                detail: format!("{event_type} is not a key event this surface forwards"),
            });
        }
        let mut params = json!({
            "type": event_type,
            "key": key,
            "code": code,
            "modifiers": modifiers,
        });
        if let Some(text) = text {
            params["text"] = json!(text);
        }
        self.connection
            .call_session(&self.cdp_session_id, "Input.dispatchKeyEvent", params)
            .await?;
        Ok(())
    }

    /// Inserts text as if typed, bypassing per-key synthesis.
    pub async fn insert_text(&self, text: &str) -> Result<(), BrowserError> {
        self.connection
            .call_session(
                &self.cdp_session_id,
                "Input.insertText",
                json!({ "text": text }),
            )
            .await?;
        Ok(())
    }

    /// Kills the browser process without going through `close`.
    ///
    /// Only for tests that need to simulate a crash: production teardown goes
    /// through the broker so the session is deregistered too.
    #[cfg(test)]
    pub(crate) async fn kill_child_for_test(&self) {
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
    }

    async fn shutdown(&self) {
        let _ = self
            .connection
            .call_session(&self.cdp_session_id, "Page.close", json!({}))
            .await;
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
        // Best effort: a profile left behind is noise, not a failure.
        let _ = std::fs::remove_dir_all(&self.profile_dir);
    }
}

/// Normalizes a user-supplied address into a URL the browser will accept.
///
/// Bare hosts and `localhost:3000` are the common agent inputs and must not be
/// rejected, but a scheme that could reach the local filesystem or execute
/// script is refused outright.
pub fn normalize_url(input: &str) -> Result<String, BrowserError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(BrowserError::Invalid {
            detail: "a URL is required".to_string(),
        });
    }
    let lowered = trimmed.to_ascii_lowercase();
    for blocked in ["javascript:", "data:", "file:", "vbscript:"] {
        if lowered.starts_with(blocked) {
            return Err(BrowserError::Invalid {
                detail: format!("{blocked} URLs are not allowed in a browser surface"),
            });
        }
    }
    if lowered == "about:blank" {
        return Ok("about:blank".to_string());
    }
    if lowered.starts_with("http://") || lowered.starts_with("https://") {
        return Ok(trimmed.to_string());
    }
    if lowered.contains("://") {
        return Err(BrowserError::Invalid {
            detail: format!("{trimmed} uses a scheme a browser surface cannot open"),
        });
    }
    Ok(format!("http://{trimmed}"))
}

/// What a caller must supply to open a session.
#[derive(Debug, Clone, Default)]
pub struct OpenBrowserRequest {
    pub url: Option<String>,
    pub owner_agent_id: Option<String>,
    pub workspace: Option<String>,
    pub viewport: Option<Viewport>,
}

/// Owns every live browser session in the app.
#[derive(Debug)]
pub struct BrowserSessionBroker {
    sessions: RwLock<HashMap<String, Arc<BrowserSession>>>,
    next_short_ref: AtomicU32,
    events: broadcast::Sender<BrowserSessionEvent>,
    profile_root: PathBuf,
}

impl Default for BrowserSessionBroker {
    fn default() -> Self {
        Self::new(std::env::temp_dir().join("wardian-browser-profiles"))
    }
}

impl BrowserSessionBroker {
    pub fn new(profile_root: PathBuf) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            sessions: RwLock::new(HashMap::new()),
            next_short_ref: AtomicU32::new(1),
            events,
            profile_root,
        }
    }

    /// Subscribes to every session's events, for forwarding to the frontend.
    pub fn subscribe(&self) -> broadcast::Receiver<BrowserSessionEvent> {
        self.events.subscribe()
    }

    /// Starts a browser, attaches to a fresh page, and registers the session.
    pub async fn open(
        &self,
        request: OpenBrowserRequest,
    ) -> Result<Arc<BrowserSession>, BrowserError> {
        // Validate the URL before paying for a browser launch.
        let target_url = match request.url.as_deref() {
            Some(url) => Some(normalize_url(url)?),
            None => None,
        };
        let binary = discover_engine().map_err(BrowserError::Engine)?;
        let browser_id = Uuid::new_v4().to_string();
        let viewport = request.viewport.unwrap_or_default();
        let profile_dir = self.profile_root.join(&browser_id);

        // Every step from here to a registered session can fail with a profile
        // already on disk and, past `launch_engine`, a live child. The child
        // dies with its dropped handle (`kill_on_drop`), but the profile
        // directory would be left behind on every failed open.
        let started = self
            .start_session(&binary, &browser_id, &profile_dir, viewport, &request)
            .await;
        let session = match started {
            Ok(session) => session,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&profile_dir);
                return Err(error);
            }
        };

        self.spawn_event_pump(Arc::clone(&session));
        self.sessions
            .write()
            .await
            .insert(browser_id.clone(), Arc::clone(&session));

        if let Some(url) = target_url {
            // A failed first load is a page outcome, not a failed open. The
            // session is already registered and its browser is running, so
            // returning Err here would strand a live browser with no handle.
            // The caller sees `load_state: failed` and can navigate again.
            if session.navigate(&url).await.is_err() {
                session.state.write().await.load_state = LoadState::Failed;
            }
        }
        let _ = self.events.send(BrowserSessionEvent::State {
            browser_id,
            summary: session.summary().await,
        });
        Ok(session)
    }

    /// Launches a browser, attaches to a fresh page, and builds the session.
    ///
    /// Split out so `open` has one place to clean up after any failure in the
    /// sequence rather than a cleanup arm per step.
    async fn start_session(
        &self,
        binary: &EngineBinary,
        browser_id: &str,
        profile_dir: &std::path::Path,
        viewport: Viewport,
        request: &OpenBrowserRequest,
    ) -> Result<Arc<BrowserSession>, BrowserError> {
        let launched = launch_engine(binary, profile_dir, viewport.width, viewport.height)
            .await
            .map_err(BrowserError::Engine)?;
        let connection = CdpConnection::connect(&launched.websocket_url).await?;

        // Size is deliberately omitted: the protocol only accepts it alongside
        // `newWindow`, and the viewport is established by the metrics override
        // below, which is what the screencast actually follows.
        let created = connection
            .call("Target.createTarget", json!({ "url": "about:blank" }))
            .await?;
        let target_id = required_str("Target.createTarget", &created, "targetId")?;
        let attached = connection
            .call(
                "Target.attachToTarget",
                json!({ "targetId": target_id, "flatten": true }),
            )
            .await?;
        let cdp_session_id = required_str("Target.attachToTarget", &attached, "sessionId")?;

        for method in ["Page.enable", "Runtime.enable", "Log.enable"] {
            connection
                .call_session(&cdp_session_id, method, json!({}))
                .await?;
        }

        let session = Arc::new(BrowserSession {
            browser_id: browser_id.to_string(),
            short_ref: self.next_short_ref.fetch_add(1, Ordering::Relaxed),
            owner_agent_id: request.owner_agent_id.clone(),
            workspace: request.workspace.clone(),
            engine: launched.kind,
            connection,
            cdp_session_id,
            profile_dir: profile_dir.to_path_buf(),
            child: Mutex::new(Some(launched.child)),
            state: RwLock::new(SessionState {
                viewport,
                ..SessionState::default()
            }),
        });
        session.set_viewport(Some(viewport)).await?;
        Ok(session)
    }

    /// Translates protocol events into session state and surface events.
    fn spawn_event_pump(&self, session: Arc<BrowserSession>) {
        let mut receiver = session.connection.subscribe();
        let events = self.events.clone();
        let cdp_session_id = session.cdp_session_id.clone();
        let browser_id = session.browser_id.clone();
        tokio::spawn(async move {
            loop {
                let event = match receiver.recv().await {
                    Ok(event) => event,
                    // A lagging receiver dropped events, and this channel
                    // carries `Page.frameNavigated` as well as frames. Rather
                    // than assume only frames were lost, invalidate every
                    // outstanding ref and resynchronize: a spurious
                    // `snapshot_stale` costs one re-snapshot, while a missed
                    // navigation would let a ref act on a different document.
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        session.state.write().await.ledger.invalidate();
                        resynchronize(&session, &events).await;
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        let _ = events.send(BrowserSessionEvent::Closed {
                            browser_id: browser_id.clone(),
                            reason: "the browser process exited".to_string(),
                        });
                        return;
                    }
                };
                // Checked before the session filter: the disconnect signal is
                // connection-scoped and carries no target session.
                if event.method == DISCONNECTED_METHOD {
                    let _ = events.send(BrowserSessionEvent::Closed {
                        browser_id: browser_id.clone(),
                        reason: "the browser process exited".to_string(),
                    });
                    return;
                }
                if event.session_id.as_deref() != Some(cdp_session_id.as_str()) {
                    continue;
                }
                handle_protocol_event(&session, &events, event).await;
            }
        });
    }

    /// Resolves `browser:N`, a full id, or an unambiguous id prefix.
    pub async fn resolve(&self, target: &str) -> Result<Arc<BrowserSession>, BrowserError> {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return Err(BrowserError::NotFound {
                target: "an empty target".to_string(),
            });
        }
        let sessions = self.sessions.read().await;
        if let Some(index) = trimmed
            .strip_prefix("browser:")
            .and_then(|rest| rest.parse::<u32>().ok())
        {
            return sessions
                .values()
                .find(|session| session.short_ref == index)
                .cloned()
                .ok_or_else(|| BrowserError::NotFound {
                    target: trimmed.to_string(),
                });
        }
        if let Some(session) = sessions.get(trimmed) {
            return Ok(Arc::clone(session));
        }
        let matches: Vec<Arc<BrowserSession>> = sessions
            .values()
            .filter(|session| session.browser_id.starts_with(trimmed))
            .cloned()
            .collect();
        match matches.len() {
            0 => Err(BrowserError::NotFound {
                target: trimmed.to_string(),
            }),
            1 => Ok(matches.into_iter().next().expect("one match")),
            _ => Err(BrowserError::Ambiguous {
                target: trimmed.to_string(),
                matches: matches
                    .iter()
                    .map(|session| session.short_ref())
                    .collect(),
            }),
        }
    }

    /// Every open session, ordered by short ref so listings are stable.
    pub async fn list(&self) -> Vec<BrowserSessionSummary> {
        let sessions = self.sessions.read().await;
        let mut ordered: Vec<Arc<BrowserSession>> = sessions.values().cloned().collect();
        drop(sessions);
        ordered.sort_by_key(|session| session.short_ref);
        let mut summaries = Vec::with_capacity(ordered.len());
        for session in ordered {
            summaries.push(session.summary().await);
        }
        summaries
    }

    /// Closes one session and stops its browser.
    pub async fn close(&self, target: &str) -> Result<String, BrowserError> {
        let session = self.resolve(target).await?;
        let browser_id = session.browser_id.clone();
        self.sessions.write().await.remove(&browser_id);
        session.shutdown().await;
        let _ = self.events.send(BrowserSessionEvent::Closed {
            browser_id: browser_id.clone(),
            reason: "closed".to_string(),
        });
        Ok(browser_id)
    }

    /// Closes every session owned by an agent that is going away.
    pub async fn close_for_agent(&self, agent_id: &str) -> Vec<String> {
        let owned: Vec<Arc<BrowserSession>> = self
            .sessions
            .read()
            .await
            .values()
            .filter(|session| session.owner_agent_id.as_deref() == Some(agent_id))
            .cloned()
            .collect();
        let mut closed = Vec::with_capacity(owned.len());
        for session in owned {
            let browser_id = session.browser_id.clone();
            self.sessions.write().await.remove(&browser_id);
            session.shutdown().await;
            let _ = self.events.send(BrowserSessionEvent::Closed {
                browser_id: browser_id.clone(),
                reason: "the owning agent stopped".to_string(),
            });
            closed.push(browser_id);
        }
        closed
    }

    /// Stops every session. Called on app exit.
    pub async fn shutdown_all(&self) {
        let sessions: Vec<Arc<BrowserSession>> =
            self.sessions.write().await.drain().map(|(_, s)| s).collect();
        for session in sessions {
            session.shutdown().await;
        }
    }
}

/// Applies one protocol event to session state and republishes what surfaces need.
async fn handle_protocol_event(
    session: &Arc<BrowserSession>,
    events: &broadcast::Sender<BrowserSessionEvent>,
    event: CdpEvent,
) {
    let browser_id = session.browser_id.clone();
    match event.method.as_str() {
        "Page.screencastFrame" => {
            let ack_id = event.params.get("sessionId").and_then(Value::as_i64);
            if let Some(ack_id) = ack_id {
                let _ = session
                    .connection
                    .call_session(
                        &session.cdp_session_id,
                        "Page.screencastFrameAck",
                        json!({ "sessionId": ack_id }),
                    )
                    .await;
            }
            let Some(data) = event.params.get("data").and_then(Value::as_str) else {
                return;
            };
            let metadata = event.params.get("metadata");
            let width = metadata
                .and_then(|value| value.get("deviceWidth"))
                .and_then(Value::as_f64)
                .unwrap_or(f64::from(DEFAULT_VIEWPORT_WIDTH)) as u32;
            let height = metadata
                .and_then(|value| value.get("deviceHeight"))
                .and_then(Value::as_f64)
                .unwrap_or(f64::from(DEFAULT_VIEWPORT_HEIGHT)) as u32;
            let _ = events.send(BrowserSessionEvent::Frame {
                browser_id,
                data: data.to_string(),
                width,
                height,
            });
        }
        // A History API or fragment navigation changes the route without a
        // frame commit. The URL has to follow it, and refs taken against the
        // previous route must not survive it.
        "Page.navigatedWithinDocument" => {
            let url = event
                .params
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            {
                let mut state = session.state.write().await;
                state.ledger.invalidate();
                if !url.is_empty() {
                    state.url = url;
                }
            }
            let _ = events.send(BrowserSessionEvent::State {
                browser_id,
                summary: session.summary().await,
            });
        }
        "Page.frameNavigated" => {
            // Only a main-frame commit invalidates refs; an iframe navigating
            // must not throw away the refs the agent just took.
            let is_main_frame = event
                .params
                .get("frame")
                .map(|frame| frame.get("parentId").is_none())
                .unwrap_or(false);
            if !is_main_frame {
                return;
            }
            let url = event
                .params
                .get("frame")
                .and_then(|frame| frame.get("url"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            {
                let mut state = session.state.write().await;
                state.ledger.invalidate();
                state.url = url;
                state.load_state = LoadState::Loading;
                state.console_error_count = 0;
                state.console.clear();
            }
            let _ = events.send(BrowserSessionEvent::State {
                browser_id,
                summary: session.summary().await,
            });
        }
        "Page.loadEventFired" | "Page.domContentEventFired" => {
            {
                let mut state = session.state.write().await;
                state.load_state = LoadState::Complete;
            }
            if let Ok(title) = session.get(PageField::Title, None).await {
                session.state.write().await.title = title;
            }
            let _ = events.send(BrowserSessionEvent::State {
                browser_id,
                summary: session.summary().await,
            });
        }
        "Runtime.consoleAPICalled" | "Log.entryAdded" => {
            let (level, text) = console_entry_from(&event);
            let is_error = level == "error";
            let entry = ConsoleEntry { level, text };
            {
                let mut state = session.state.write().await;
                if is_error {
                    state.console_error_count += 1;
                }
                if state.console.len() >= CONSOLE_BUFFER {
                    state.console.pop_front();
                }
                state.console.push_back(entry.clone());
            }
            let _ = events.send(BrowserSessionEvent::Console { browser_id, entry });
        }
        _ => {}
    }
}

/// Re-reads the page's own view of itself after events were dropped.
///
/// Cheaper and more truthful than guessing which events were lost.
async fn resynchronize(
    session: &Arc<BrowserSession>,
    events: &broadcast::Sender<BrowserSessionEvent>,
) {
    if let Ok(url) = session.get(PageField::Url, None).await {
        session.state.write().await.url = url;
    }
    if let Ok(title) = session.get(PageField::Title, None).await {
        session.state.write().await.title = title;
    }
    let _ = events.send(BrowserSessionEvent::State {
        browser_id: session.browser_id().to_string(),
        summary: session.summary().await,
    });
}

/// Flattens either console event shape into one level/text pair.
pub(crate) fn console_entry_from(event: &CdpEvent) -> (String, String) {
    if event.method == "Log.entryAdded" {
        let entry = event.params.get("entry");
        let level = entry
            .and_then(|value| value.get("level"))
            .and_then(Value::as_str)
            .unwrap_or("info")
            .to_string();
        let text = entry
            .and_then(|value| value.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        return (normalize_console_level(&level), text);
    }
    let level = event
        .params
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("log")
        .to_string();
    let text = event
        .params
        .get("args")
        .and_then(Value::as_array)
        .map(|args| {
            args.iter()
                .map(|arg| {
                    arg.get("value")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| arg.get("value").map(std::string::ToString::to_string))
                        .or_else(|| {
                            arg.get("description").and_then(Value::as_str).map(str::to_string)
                        })
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    (normalize_console_level(&level), text)
}

/// Collapses the protocol's several severity vocabularies into three levels.
pub(crate) fn normalize_console_level(level: &str) -> String {
    match level {
        "error" | "assert" | "severe" => "error",
        "warning" | "warn" => "warning",
        _ => "info",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https_unchanged() {
        assert_eq!(normalize_url("https://example.com/a").unwrap(), "https://example.com/a");
        assert_eq!(normalize_url("  http://localhost:3000 ").unwrap(), "http://localhost:3000");
    }

    #[test]
    fn promotes_a_bare_host_to_http() {
        assert_eq!(normalize_url("localhost:5173").unwrap(), "http://localhost:5173");
        assert_eq!(normalize_url("example.com").unwrap(), "http://example.com");
    }

    #[test]
    fn refuses_schemes_that_could_reach_the_host_or_run_script() {
        for blocked in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,<h1>x",
            "file:///etc/passwd",
            "vbscript:msgbox",
        ] {
            let error = normalize_url(blocked).expect_err("blocked");
            assert_eq!(error.code(), "browser_invalid_request", "{blocked}");
        }
    }

    #[test]
    fn refuses_an_unknown_scheme_rather_than_prefixing_it() {
        let error = normalize_url("ftp://example.com").expect_err("blocked");
        assert!(error.to_string().contains("cannot open"));
    }

    #[test]
    fn allows_about_blank_as_the_one_non_http_target() {
        assert_eq!(normalize_url("about:blank").unwrap(), "about:blank");
    }

    #[test]
    fn refuses_an_empty_url() {
        assert_eq!(
            normalize_url("   ").expect_err("empty").code(),
            "browser_invalid_request"
        );
    }


    #[test]
    fn wait_conditions_compile_to_javascript_that_quotes_their_input() {
        let predicate = WaitCondition::Selector("#a[data-x='1']".to_string())
            .predicate()
            .expect("predicate");
        assert!(predicate.contains(r##"document.querySelector("#a[data-x='1']")"##));
        let text = WaitCondition::Text("say \"hi\"".to_string())
            .predicate()
            .expect("predicate");
        assert!(text.contains(r#"\"hi\""#), "quotes must be escaped: {text}");
    }

    #[test]
    fn waiting_for_the_failed_load_state_has_no_predicate() {
        assert_eq!(WaitCondition::LoadState(LoadState::Failed).predicate(), None);
    }

    #[test]
    fn wait_conditions_describe_themselves_for_the_timeout_message() {
        assert_eq!(
            WaitCondition::LoadState(LoadState::Complete).describe(),
            "load state complete"
        );
        assert_eq!(
            WaitCondition::UrlContains("/dashboard".to_string()).describe(),
            r#"url containing "/dashboard""#
        );
    }

    #[test]
    fn page_fields_parse_from_their_cli_names() {
        assert_eq!(PageField::parse("url"), Some(PageField::Url));
        assert_eq!(PageField::parse("html"), Some(PageField::Html));
        assert_eq!(PageField::parse("screenshot"), None);
    }

    #[test]
    fn console_levels_collapse_to_three_buckets() {
        assert_eq!(normalize_console_level("severe"), "error");
        assert_eq!(normalize_console_level("assert"), "error");
        assert_eq!(normalize_console_level("warn"), "warning");
        assert_eq!(normalize_console_level("debug"), "info");
    }

    #[test]
    fn reads_a_console_api_event_into_a_level_and_text() {
        let event = CdpEvent {
            session_id: Some("s".to_string()),
            method: "Runtime.consoleAPICalled".to_string(),
            params: json!({
                "type": "error",
                "args": [{ "value": "boom" }, { "value": "again" }]
            }),
        };
        assert_eq!(console_entry_from(&event), ("error".to_string(), "boom again".to_string()));
    }

    #[test]
    fn reads_a_log_entry_event_into_a_level_and_text() {
        let event = CdpEvent {
            session_id: Some("s".to_string()),
            method: "Log.entryAdded".to_string(),
            params: json!({ "entry": { "level": "warning", "text": "deprecated" } }),
        };
        assert_eq!(
            console_entry_from(&event),
            ("warning".to_string(), "deprecated".to_string())
        );
    }

    #[test]
    fn error_codes_are_stable_for_json_consumers() {
        assert_eq!(
            BrowserError::NotFound { target: "browser:9".to_string() }.code(),
            "browser_not_found"
        );
        assert_eq!(
            BrowserError::WaitTimeout {
                condition: "x".to_string(),
                timeout_ms: 1
            }
            .code(),
            "browser_wait_timeout"
        );
        assert_eq!(
            BrowserError::Ref(RefError::NoSnapshot).code(),
            "snapshot_missing",
            "ref failures keep their own code rather than being flattened"
        );
    }

    #[test]
    fn a_not_found_error_points_at_the_listing_command() {
        let message = BrowserError::NotFound {
            target: "browser:9".to_string(),
        }
        .to_string();
        assert!(message.contains("wardian browser list"));
    }

    #[tokio::test]
    async fn resolving_against_an_empty_broker_reports_not_found() {
        let broker = BrowserSessionBroker::new(std::env::temp_dir().join("wardian-test-profiles"));
        assert_eq!(
            broker.resolve("browser:1").await.expect_err("empty").code(),
            "browser_not_found"
        );
        assert_eq!(
            broker.resolve("  ").await.expect_err("empty").code(),
            "browser_not_found"
        );
        assert!(broker.list().await.is_empty());
    }

    #[test]
    fn a_session_event_always_names_its_session() {
        let event = BrowserSessionEvent::Closed {
            browser_id: "abc".to_string(),
            reason: "closed".to_string(),
        };
        assert_eq!(event.browser_id(), "abc");
    }

    #[test]
    fn session_events_serialize_with_a_discriminating_kind() {
        let event = BrowserSessionEvent::Frame {
            browser_id: "abc".to_string(),
            data: "AAA".to_string(),
            width: 100,
            height: 50,
        };
        let encoded = serde_json::to_value(&event).expect("serialize");
        assert_eq!(encoded["kind"], "frame");
        assert_eq!(encoded["browser_id"], "abc");
    }
}

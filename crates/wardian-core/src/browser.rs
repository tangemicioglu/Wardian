//! Wire types shared by the browser surface, the control plane, and the CLI.
//!
//! Runtime behavior lives in the app; this module holds only the shapes both
//! sides must agree on, so `wardian browser` and the workbench surface can
//! never drift apart.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Which Chromium family backs a session. Diagnostics only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    Edge,
    Chrome,
    Chromium,
    Brave,
    Custom,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Edge => "edge",
            EngineKind::Chrome => "chrome",
            EngineKind::Chromium => "chromium",
            EngineKind::Brave => "brave",
            EngineKind::Custom => "custom",
        }
    }
}

/// Where a page is in its load cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadState {
    #[default]
    Idle,
    Loading,
    Complete,
    Failed,
}

impl LoadState {
    pub fn as_str(self) -> &'static str {
        match self {
            LoadState::Idle => "idle",
            LoadState::Loading => "loading",
            LoadState::Complete => "complete",
            LoadState::Failed => "failed",
        }
    }

    /// Parses the `--load-state` CLI value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "idle" => Some(LoadState::Idle),
            "loading" => Some(LoadState::Loading),
            "complete" => Some(LoadState::Complete),
            "failed" => Some(LoadState::Failed),
            _ => None,
        }
    }
}

/// Default viewport for a new session.
pub const DEFAULT_VIEWPORT_WIDTH: u32 = 1280;
pub const DEFAULT_VIEWPORT_HEIGHT: u32 = 800;

/// Viewport the page is rendered at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: DEFAULT_VIEWPORT_WIDTH,
            height: DEFAULT_VIEWPORT_HEIGHT,
        }
    }
}

/// One console entry captured from the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleEntry {
    pub level: String,
    pub text: String,
}

/// A page dialog waiting to be answered.
///
/// `alert`, `confirm`, `prompt`, and `beforeunload` all stop the page until
/// someone answers. The page cannot proceed while this is set, so it is part
/// of the session's public description rather than a surface-only concern:
/// an agent that finds its `wait` timing out deserves to see why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDialog {
    /// `alert`, `confirm`, `prompt`, or `beforeunload`.
    pub kind: String,
    pub message: String,
    /// What a `prompt` starts with; empty for every other kind.
    #[serde(default)]
    pub default_prompt: String,
}

/// The externally visible description of a browser session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSessionSummary {
    pub browser_id: String,
    /// Short ref an agent or human can type, e.g. `browser:3`.
    pub short_ref: String,
    pub url: String,
    pub title: String,
    pub load_state: LoadState,
    pub viewport: Viewport,
    pub engine: EngineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default)]
    pub console_error_count: usize,
    /// Recorded requests that failed outright or answered 4xx/5xx.
    #[serde(default)]
    pub network_failure_count: usize,
    /// True while this session is presenting a page it opened in a popup.
    ///
    /// The surface has one viewport, so a popup is presented in place of its
    /// opener rather than beside it. Everything else — `url`, `title`,
    /// snapshots, actions — already describes whatever is presented, so this
    /// is the one bit that says the opener is still behind it.
    #[serde(default)]
    pub popup: bool,
    /// The dialog blocking the page, if one is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialog: Option<BrowserDialog>,
}

/// Hard ceiling on elements in one snapshot, so a pathological page cannot
/// flood an agent's context.
pub const MAX_SNAPSHOT_ELEMENTS: usize = 400;
/// Hard ceiling on characters in any single accessible name or value.
pub const MAX_SNAPSHOT_FIELD_CHARS: usize = 160;

/// One referenced element in a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotElement {
    /// Stable within the snapshot generation that produced it, e.g. `e12`.
    pub element_ref: String,
    /// ARIA role when present, otherwise the lowercased tag name.
    pub role: String,
    /// Accessible name: label, aria-label, placeholder, alt, or text.
    pub name: String,
    /// Current value for inputs; empty for everything else.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
}

fn default_true() -> bool {
    true
}

/// A page snapshot plus the identity that makes its refs actionable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageSnapshot {
    pub generation: u64,
    pub url: String,
    pub title: String,
    pub interactive_only: bool,
    pub elements: Vec<SnapshotElement>,
    /// True when the page had more elements than [`MAX_SNAPSHOT_ELEMENTS`].
    #[serde(default)]
    pub truncated: bool,
}

/// Result of a DOM action, optionally carrying the re-snapshot that followed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserActionResult {
    pub browser_id: String,
    pub action: String,
    pub element_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<PageSnapshot>,
}

/// Result of reading one field off the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserGetResult {
    pub browser_id: String,
    pub field: String,
    pub value: String,
}

/// Result of writing a screenshot to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserScreenshotResult {
    pub browser_id: String,
    pub path: String,
    pub full_page: bool,
}

// ---------------------------------------------------------------------------
// Phase 3: introspection
// ---------------------------------------------------------------------------

/// Requests retained per session. A page load is a few hundred requests, so one
/// full load always fits.
pub const NETWORK_BUFFER: usize = 500;
/// Ceiling on a stored URL. `data:` URIs are otherwise unbounded.
pub const MAX_NETWORK_URL_CHARS: usize = 1024;
/// Ceiling on stored headers, per direction.
pub const MAX_NETWORK_HEADERS: usize = 32;
/// Ceiling on any single stored header value.
pub const MAX_NETWORK_HEADER_CHARS: usize = 512;
/// Ceiling on a response body read back through `network <id> --body`.
pub const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024;
/// Ceiling on a whole storage listing.
pub const MAX_STORAGE_BYTES: usize = 64 * 1024;
/// Ceiling on any single storage value.
pub const MAX_STORAGE_VALUE_CHARS: usize = 2048;
/// How long a closed session's downloads stay on disk.
pub const DOWNLOAD_RETENTION_DAYS: u64 = 7;

/// One request the page made, as the ledger records it.
///
/// Headers are deliberately absent: a listing renders one line per request, and
/// carrying every header through it would dwarf the thing being listed. They
/// live on [`NetworkRequestDetail`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkEntry {
    /// Protocol request id, the handle `network <id>` takes.
    pub request_id: String,
    pub method: String,
    pub url: String,
    /// `document`, `xhr`, `fetch`, `script`, … as the protocol reports it.
    pub resource_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_data_length: Option<u64>,
    /// Why the request never completed. Present only for failures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(default)]
    pub from_cache: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// True when the URL hit [`MAX_NETWORK_URL_CHARS`].
    #[serde(default)]
    pub url_truncated: bool,
}

impl NetworkEntry {
    /// True when the request failed outright or answered 4xx/5xx.
    pub fn is_failure(&self) -> bool {
        self.failure.is_some() || self.status.is_some_and(|status| status >= 400)
    }
}

/// A response body read back on demand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkBody {
    pub text: String,
    /// True when the body was binary and `text` is its base64 encoding.
    #[serde(default)]
    pub base64_encoded: bool,
    /// True when the body hit [`MAX_RESPONSE_BODY_BYTES`].
    #[serde(default)]
    pub truncated: bool,
}

/// Everything the ledger holds about one request, plus an optional live body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkRequestDetail {
    pub entry: NetworkEntry,
    #[serde(default)]
    pub request_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub response_headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<NetworkBody>,
    /// Why the body could not be read. A body outlives its request only while
    /// the browser's own buffer holds it, so this is an ordinary outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_error: Option<String>,
}

/// Matches an HTTP status exactly, or by its leading digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusFilter {
    Exact(u16),
    /// `2xx` — the leading digit, 1 through 5.
    Class(u8),
}

impl StatusFilter {
    /// Parses `404` or `2xx`.
    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        let lowered = trimmed.to_ascii_lowercase();
        if let Some(leading) = lowered.strip_suffix("xx") {
            let class: u8 = leading.parse().ok()?;
            return (1..=5).contains(&class).then_some(StatusFilter::Class(class));
        }
        let exact: u16 = trimmed.parse().ok()?;
        (100..=599).contains(&exact).then_some(StatusFilter::Exact(exact))
    }

    pub fn matches(self, status: u16) -> bool {
        match self {
            StatusFilter::Exact(expected) => status == expected,
            StatusFilter::Class(class) => status / 100 == u16::from(class),
        }
    }
}

/// Which records a `network` listing should return.
///
/// Applied in the backend rather than the CLI so `--limit` bounds what crosses
/// the wire, not just what is printed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkFilter {
    /// Case-insensitive substring of the URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusFilter>,
    /// Any of these resource types. Empty means every type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_types: Vec<String>,
    #[serde(default)]
    pub failed_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

impl NetworkFilter {
    /// True when one record survives every predicate that was set.
    pub fn matches(&self, entry: &NetworkEntry) -> bool {
        if let Some(text) = self.text.as_deref() {
            if !entry.url.to_lowercase().contains(&text.to_lowercase()) {
                return false;
            }
        }
        if let Some(method) = self.method.as_deref() {
            if !entry.method.eq_ignore_ascii_case(method) {
                return false;
            }
        }
        if let Some(status) = self.status {
            match entry.status {
                Some(actual) if status.matches(actual) => {}
                _ => return false,
            }
        }
        if !self.resource_types.is_empty()
            && !self
                .resource_types
                .iter()
                .any(|kind| entry.resource_type.eq_ignore_ascii_case(kind))
        {
            return false;
        }
        if self.failed_only && !entry.is_failure() {
            return false;
        }
        true
    }

    /// Applies every predicate, then keeps the most recent `limit` records.
    ///
    /// Trimming from the front rather than the back: the newest requests are
    /// the ones an agent just triggered.
    pub fn apply(&self, entries: &[NetworkEntry]) -> Vec<NetworkEntry> {
        let mut kept: Vec<NetworkEntry> = entries
            .iter()
            .filter(|entry| self.matches(entry))
            .cloned()
            .collect();
        if let Some(limit) = self.limit {
            if kept.len() > limit {
                kept.drain(..kept.len() - limit);
            }
        }
        kept
    }
}

/// One cookie held by the session's isolated profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_site: Option<String>,
    /// Seconds since the epoch. Absent for a session cookie.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<f64>,
}

/// Which web-storage area a command addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageArea {
    Local,
    Session,
}

impl StorageArea {
    pub fn as_str(self) -> &'static str {
        match self {
            StorageArea::Local => "local",
            StorageArea::Session => "session",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "local" | "localStorage" => Some(StorageArea::Local),
            "session" | "sessionStorage" => Some(StorageArea::Session),
            _ => None,
        }
    }

    /// The DOM accessor this area is read and written through.
    pub fn accessor(self) -> &'static str {
        match self {
            StorageArea::Local => "localStorage",
            StorageArea::Session => "sessionStorage",
        }
    }
}

/// One key/value pair in a storage area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageEntry {
    pub key: String,
    pub value: String,
    /// True when the value hit [`MAX_STORAGE_VALUE_CHARS`].
    #[serde(default)]
    pub truncated: bool,
}

/// A whole storage area as read from one origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageSnapshot {
    pub area: StorageArea,
    pub origin: String,
    pub entries: Vec<StorageEntry>,
    /// True when the listing stopped at [`MAX_STORAGE_BYTES`].
    #[serde(default)]
    pub truncated: bool,
}

/// One file the page downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadRecord {
    /// The browser's download GUID, which also names the file until it settles.
    pub guid: String,
    pub url: String,
    pub suggested_filename: String,
    /// `in_progress`, `completed`, or `canceled`.
    pub state: String,
    #[serde(default)]
    pub received_bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
    /// Where the file actually is, once it has settled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// What a `network` invocation asks the session to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum NetworkAction {
    List {
        #[serde(default)]
        filter: NetworkFilter,
    },
    Detail {
        request_id: String,
        /// Read the response body back from the browser as well.
        #[serde(default)]
        body: bool,
    },
    Clear,
}

/// What a `cookies` invocation asks the session to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CookieAction {
    List {
        /// Every cookie in the browser context, not only the page's.
        #[serde(default)]
        all: bool,
    },
    Set {
        name: String,
        value: String,
        /// Defaults to the page's current URL.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        domain: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default)]
        secure: bool,
        #[serde(default)]
        http_only: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        same_site: Option<String>,
        /// Whole seconds since the epoch. Sub-second cookie expiry is not a
        /// thing anyone needs, and an integer keeps the request comparable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires: Option<i64>,
    },
    Delete {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        domain: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    Clear,
}

/// What a `storage local|session` invocation asks the session to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum StorageAction {
    /// One key, or the whole area when no key is given.
    Get {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    Set {
        key: String,
        value: String,
    },
    Remove {
        key: String,
    },
    Clear,
}

/// What a `network` verb produced: one request, many, or nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum NetworkOutcome {
    List {
        entries: Vec<NetworkEntry>,
    },
    /// Boxed: a detail carries both header maps and a body, which would
    /// otherwise make every listing pay for the largest variant.
    Detail {
        detail: Box<NetworkRequestDetail>,
    },
    Cleared,
}

/// What a `storage` verb produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum StorageOutcome {
    Snapshot {
        snapshot: StorageSnapshot,
    },
    /// One key's value, absent when the area does not hold it.
    Value {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
    },
    Applied,
}

/// Renders one network record as a single scannable line.
pub fn render_network_line(entry: &NetworkEntry) -> String {
    let status = match (&entry.failure, entry.status) {
        (Some(_), _) => "FAIL".to_string(),
        (None, Some(status)) => status.to_string(),
        (None, None) => "…".to_string(),
    };
    let mut line = format!(
        "{}  {:<6} {:<4} {:<10} {}",
        entry.request_id, entry.method, status, entry.resource_type, entry.url
    );
    if entry.url_truncated {
        line.push('…');
    }
    if let Some(duration) = entry.duration_ms {
        line.push_str(&format!("  {duration}ms"));
    }
    if entry.from_cache {
        line.push_str("  cached");
    }
    if let Some(failure) = entry.failure.as_deref() {
        line.push_str(&format!("  {failure}"));
    }
    line
}

/// Renders a full request detail, headers included.
pub fn render_network_detail(detail: &NetworkRequestDetail) -> String {
    let mut lines = vec![render_network_line(&detail.entry)];
    if let Some(length) = detail.entry.encoded_data_length {
        lines.push(format!("bytes: {length}"));
    }
    if let Some(mime) = detail.entry.mime_type.as_deref() {
        lines.push(format!("mime: {mime}"));
    }
    for (label, headers) in [
        ("request headers", &detail.request_headers),
        ("response headers", &detail.response_headers),
    ] {
        if headers.is_empty() {
            continue;
        }
        lines.push(format!("{label}:"));
        for (name, value) in headers {
            lines.push(format!("  {name}: {value}"));
        }
    }
    if let Some(body) = detail.body.as_ref() {
        lines.push(if body.base64_encoded {
            "body (base64):".to_string()
        } else {
            "body:".to_string()
        });
        lines.push(body.text.clone());
        if body.truncated {
            lines.push(format!("… truncated at {MAX_RESPONSE_BODY_BYTES} bytes"));
        }
    }
    if let Some(error) = detail.body_error.as_deref() {
        lines.push(format!("body unavailable: {error}"));
    }
    lines.join("\n")
}

/// Renders one cookie as `name=value` plus the attributes that constrain it.
pub fn render_cookie_line(cookie: &BrowserCookie) -> String {
    let mut line = format!(
        "{}={}  {}{}",
        cookie.name, cookie.value, cookie.domain, cookie.path
    );
    if cookie.secure {
        line.push_str("  secure");
    }
    if cookie.http_only {
        line.push_str("  httponly");
    }
    if let Some(same_site) = cookie.same_site.as_deref() {
        line.push_str(&format!("  samesite={same_site}"));
    }
    match cookie.expires {
        Some(expires) => line.push_str(&format!("  expires={expires}")),
        None => line.push_str("  session"),
    }
    line
}

/// Renders a storage area as one `key=value` per line.
pub fn render_storage(snapshot: &StorageSnapshot) -> String {
    if snapshot.entries.is_empty() {
        return format!(
            "{}Storage is empty for {}",
            snapshot.area.as_str(),
            snapshot.origin
        );
    }
    let mut lines: Vec<String> = snapshot
        .entries
        .iter()
        .map(|entry| {
            let suffix = if entry.truncated { "…" } else { "" };
            format!("{}={}{suffix}", entry.key, entry.value)
        })
        .collect();
    if snapshot.truncated {
        lines.push(format!("… truncated at {MAX_STORAGE_BYTES} bytes"));
    }
    lines.join("\n")
}

/// Renders one download with its progress and resolved path.
pub fn render_download_line(record: &DownloadRecord) -> String {
    let progress = if record.total_bytes > 0 {
        format!("{}/{}", record.received_bytes, record.total_bytes)
    } else {
        record.received_bytes.to_string()
    };
    format!(
        "{:<12} {:<12} {}  {}",
        record.state,
        progress,
        record.path.as_deref().unwrap_or(&record.suggested_filename),
        record.url
    )
}

/// Renders a snapshot as the compact listing the CLI prints.
pub fn render_snapshot(snapshot: &PageSnapshot) -> String {
    let mut lines = vec![
        format!("url: {}", snapshot.url),
        format!("title: {}", snapshot.title),
        format!("generation: {}", snapshot.generation),
    ];
    for element in &snapshot.elements {
        let mut line = format!(
            "{}  {}  {}",
            element.element_ref, element.role, element.name
        );
        if !element.value.is_empty() {
            line.push_str(&format!("  value={:?}", element.value));
        }
        if let Some(checked) = element.checked {
            line.push_str(&format!("  checked={checked}"));
        }
        if !element.enabled {
            line.push_str("  disabled");
        }
        lines.push(line);
    }
    if snapshot.truncated {
        lines.push(format!(
            "… truncated at {MAX_SNAPSHOT_ELEMENTS} elements; narrow the page or use --interactive"
        ));
    }
    lines.join("\n")
}

/// Renders a session listing as one aligned line per session.
pub fn render_session_line(summary: &BrowserSessionSummary) -> String {
    let owner = summary
        .owner_agent_id
        .as_deref()
        .map(|agent| format!("  agent={agent}"))
        .unwrap_or_default();
    let errors = if summary.console_error_count > 0 {
        format!("  console_errors={}", summary.console_error_count)
    } else {
        String::new()
    };
    let failures = if summary.network_failure_count > 0 {
        format!("  network_failures={}", summary.network_failure_count)
    } else {
        String::new()
    };
    // Both say the address alone is not the whole story: a popup means the
    // opener is still behind what this line describes, and a dialog means the
    // page is stopped and every later call will time out until it is answered.
    let popup = if summary.popup { "  popup" } else { "" };
    let dialog = summary
        .dialog
        .as_ref()
        .map(|dialog| format!("  dialog={}", dialog.kind))
        .unwrap_or_default();
    format!(
        "{}  {}  {}  {}{owner}{errors}{failures}{popup}{dialog}",
        summary.short_ref,
        summary.load_state.as_str(),
        if summary.url.is_empty() {
            "about:blank"
        } else {
            &summary.url
        },
        if summary.title.is_empty() {
            "(untitled)"
        } else {
            &summary.title
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_states_round_trip_through_their_cli_names() {
        for state in [
            LoadState::Idle,
            LoadState::Loading,
            LoadState::Complete,
            LoadState::Failed,
        ] {
            assert_eq!(LoadState::parse(state.as_str()), Some(state));
        }
        assert_eq!(LoadState::parse("nonsense"), None);
    }

    #[test]
    fn an_engine_kind_serializes_to_its_wire_name() {
        for kind in [
            EngineKind::Edge,
            EngineKind::Chrome,
            EngineKind::Chromium,
            EngineKind::Brave,
            EngineKind::Custom,
        ] {
            let encoded = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(encoded, format!("\"{}\"", kind.as_str()));
        }
    }

    #[test]
    fn the_default_viewport_matches_the_documented_constants() {
        let viewport = Viewport::default();
        assert_eq!(viewport.width, DEFAULT_VIEWPORT_WIDTH);
        assert_eq!(viewport.height, DEFAULT_VIEWPORT_HEIGHT);
    }

    #[test]
    fn a_summary_round_trips_through_json() {
        let summary = BrowserSessionSummary {
            browser_id: "b1".to_string(),
            short_ref: "browser:1".to_string(),
            url: "https://example.com/".to_string(),
            title: "Example".to_string(),
            load_state: LoadState::Complete,
            viewport: Viewport::default(),
            engine: EngineKind::Edge,
            owner_agent_id: Some("agent-1".to_string()),
            workspace: None,
            console_error_count: 2,
            network_failure_count: 1,
            popup: false,
            dialog: None,
        };
        let encoded = serde_json::to_string(&summary).expect("serialize");
        let decoded: BrowserSessionSummary = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, summary);
        assert!(!encoded.contains("workspace"), "absent fields stay off the wire");
    }

    #[test]
    fn renders_a_session_line_with_the_short_ref_first() {
        let summary = BrowserSessionSummary {
            browser_id: "b1".to_string(),
            short_ref: "browser:2".to_string(),
            url: "https://example.com/".to_string(),
            title: "Example".to_string(),
            load_state: LoadState::Complete,
            viewport: Viewport::default(),
            engine: EngineKind::Edge,
            owner_agent_id: Some("agent-1".to_string()),
            workspace: None,
            console_error_count: 3,
            network_failure_count: 2,
            popup: true,
            dialog: Some(BrowserDialog {
                kind: "confirm".to_string(),
                message: "proceed?".to_string(),
                default_prompt: String::new(),
            }),
        };
        let line = render_session_line(&summary);
        assert!(line.starts_with("browser:2  complete  https://example.com/  Example"));
        assert!(line.contains("agent=agent-1"));
        assert!(line.contains("console_errors=3"));
        // A line that showed only the popup's address would look like an
        // ordinary navigation, and one that hid the dialog would leave the
        // reader to guess why the page stopped answering.
        assert!(line.contains("  popup"));
        assert!(line.contains("dialog=confirm"));
    }

    #[test]
    fn renders_a_blank_session_without_empty_columns() {
        let summary = BrowserSessionSummary {
            browser_id: "b1".to_string(),
            short_ref: "browser:1".to_string(),
            url: String::new(),
            title: String::new(),
            load_state: LoadState::Idle,
            viewport: Viewport::default(),
            engine: EngineKind::Edge,
            owner_agent_id: None,
            workspace: None,
            console_error_count: 0,
            network_failure_count: 0,
            popup: false,
            dialog: None,
        };
        let line = render_session_line(&summary);
        assert!(line.contains("about:blank"));
        assert!(line.contains("(untitled)"));
        assert!(!line.contains("agent="));
        assert!(!line.contains("console_errors"));
        assert!(!line.contains("popup"));
        assert!(!line.contains("dialog"));
    }

    #[test]
    fn renders_a_snapshot_as_one_line_per_ref() {
        let snapshot = PageSnapshot {
            generation: 2,
            url: "https://example.com/".to_string(),
            title: "Example".to_string(),
            interactive_only: true,
            truncated: false,
            elements: vec![SnapshotElement {
                element_ref: "e1".to_string(),
                role: "textbox".to_string(),
                name: "Search".to_string(),
                value: "hello".to_string(),
                enabled: true,
                checked: None,
            }],
        };
        let rendered = render_snapshot(&snapshot);
        assert!(rendered.contains("generation: 2"));
        assert!(rendered.contains("e1  textbox  Search  value=\"hello\""));
    }

    fn entry(request_id: &str, method: &str, url: &str, status: Option<u16>) -> NetworkEntry {
        NetworkEntry {
            request_id: request_id.to_string(),
            method: method.to_string(),
            url: url.to_string(),
            resource_type: "xhr".to_string(),
            status,
            mime_type: None,
            encoded_data_length: None,
            failure: None,
            from_cache: false,
            duration_ms: None,
            url_truncated: false,
        }
    }

    #[test]
    fn a_status_filter_reads_both_an_exact_code_and_a_class() {
        assert_eq!(StatusFilter::parse("404"), Some(StatusFilter::Exact(404)));
        assert_eq!(StatusFilter::parse("2xx"), Some(StatusFilter::Class(2)));
        assert_eq!(StatusFilter::parse("5XX"), Some(StatusFilter::Class(5)));
        assert_eq!(StatusFilter::parse("  301 "), Some(StatusFilter::Exact(301)));
    }

    #[test]
    fn a_status_filter_refuses_codes_no_response_can_carry() {
        for nonsense in ["", "0xx", "6xx", "99", "600", "abc", "xx"] {
            assert_eq!(StatusFilter::parse(nonsense), None, "{nonsense}");
        }
    }

    #[test]
    fn a_status_class_matches_only_its_own_hundreds() {
        let class = StatusFilter::Class(2);
        assert!(class.matches(200));
        assert!(class.matches(204));
        assert!(!class.matches(404));
        assert!(StatusFilter::Exact(404).matches(404));
        assert!(!StatusFilter::Exact(404).matches(400));
    }

    #[test]
    fn an_empty_filter_keeps_every_record() {
        let entries = vec![entry("1", "GET", "https://a/", Some(200))];
        assert_eq!(NetworkFilter::default().apply(&entries), entries);
    }

    #[test]
    fn a_filter_matches_urls_and_methods_without_regard_to_case() {
        let entries = vec![
            entry("1", "GET", "https://example.com/API/users", Some(200)),
            entry("2", "post", "https://example.com/login", Some(200)),
        ];
        let filter = NetworkFilter {
            text: Some("api".to_string()),
            ..NetworkFilter::default()
        };
        assert_eq!(filter.apply(&entries).len(), 1);

        let filter = NetworkFilter {
            method: Some("POST".to_string()),
            ..NetworkFilter::default()
        };
        assert_eq!(filter.apply(&entries)[0].request_id, "2");
    }

    #[test]
    fn a_status_predicate_excludes_records_that_never_answered() {
        let entries = vec![entry("1", "GET", "https://a/", None)];
        let filter = NetworkFilter {
            status: Some(StatusFilter::Class(2)),
            ..NetworkFilter::default()
        };
        assert!(filter.apply(&entries).is_empty());
    }

    #[test]
    fn failed_only_covers_both_a_transport_failure_and_an_error_status() {
        let mut refused = entry("1", "GET", "https://a/", None);
        refused.failure = Some("net::ERR_CONNECTION_REFUSED".to_string());
        let entries = vec![
            refused,
            entry("2", "GET", "https://b/", Some(500)),
            entry("3", "GET", "https://c/", Some(200)),
        ];
        let filter = NetworkFilter {
            failed_only: true,
            ..NetworkFilter::default()
        };
        let kept = filter.apply(&entries);
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().all(|entry| entry.request_id != "3"));
    }

    #[test]
    fn a_resource_type_predicate_accepts_any_of_the_listed_types() {
        let mut script = entry("1", "GET", "https://a/", Some(200));
        script.resource_type = "script".to_string();
        let entries = vec![script, entry("2", "GET", "https://b/", Some(200))];
        let filter = NetworkFilter {
            resource_types: vec!["xhr".to_string(), "fetch".to_string()],
            ..NetworkFilter::default()
        };
        assert_eq!(filter.apply(&entries)[0].request_id, "2");
    }

    #[test]
    fn a_limit_keeps_the_most_recent_records_not_the_oldest() {
        let entries: Vec<NetworkEntry> = (1..=5)
            .map(|index| entry(&index.to_string(), "GET", "https://a/", Some(200)))
            .collect();
        let filter = NetworkFilter {
            limit: Some(2),
            ..NetworkFilter::default()
        };
        let kept = filter.apply(&entries);
        assert_eq!(
            kept.iter()
                .map(|entry| entry.request_id.as_str())
                .collect::<Vec<_>>(),
            vec!["4", "5"]
        );
    }

    #[test]
    fn a_limit_larger_than_the_ledger_is_not_an_error() {
        let entries = vec![entry("1", "GET", "https://a/", Some(200))];
        let filter = NetworkFilter {
            limit: Some(50),
            ..NetworkFilter::default()
        };
        assert_eq!(filter.apply(&entries).len(), 1);
    }

    #[test]
    fn a_network_line_leads_with_the_id_status_and_url() {
        let mut record = entry("42.1", "GET", "https://example.com/api", Some(200));
        record.duration_ms = Some(37);
        let line = render_network_line(&record);
        assert!(line.starts_with("42.1  GET    200 "));
        assert!(line.contains("https://example.com/api"));
        assert!(line.contains("37ms"));
    }

    #[test]
    fn a_failed_request_renders_as_fail_with_its_reason() {
        let mut record = entry("7", "GET", "https://example.com/", None);
        record.failure = Some("net::ERR_NAME_NOT_RESOLVED".to_string());
        let line = render_network_line(&record);
        assert!(line.contains("FAIL"));
        assert!(line.contains("net::ERR_NAME_NOT_RESOLVED"));
    }

    #[test]
    fn a_request_still_in_flight_renders_without_a_status() {
        let line = render_network_line(&entry("7", "GET", "https://example.com/", None));
        assert!(line.contains('…'));
        assert!(!line.contains("FAIL"));
    }

    #[test]
    fn a_detail_rendering_labels_both_header_directions() {
        let detail = NetworkRequestDetail {
            entry: entry("1", "POST", "https://example.com/login", Some(401)),
            request_headers: BTreeMap::from([(
                "authorization".to_string(),
                "Bearer abc".to_string(),
            )]),
            response_headers: BTreeMap::from([(
                "content-type".to_string(),
                "application/json".to_string(),
            )]),
            body: Some(NetworkBody {
                text: "{\"error\":\"nope\"}".to_string(),
                base64_encoded: false,
                truncated: false,
            }),
            body_error: None,
        };
        let rendered = render_network_detail(&detail);
        assert!(rendered.contains("request headers:"));
        assert!(rendered.contains("  authorization: Bearer abc"));
        assert!(rendered.contains("response headers:"));
        assert!(rendered.contains("body:"));
        assert!(rendered.contains("{\"error\":\"nope\"}"));
    }

    #[test]
    fn a_detail_says_why_a_body_could_not_be_read() {
        let detail = NetworkRequestDetail {
            entry: entry("1", "GET", "https://example.com/", Some(200)),
            request_headers: BTreeMap::new(),
            response_headers: BTreeMap::new(),
            body: None,
            body_error: Some("the browser no longer holds this response".to_string()),
        };
        let rendered = render_network_detail(&detail);
        assert!(rendered.contains("body unavailable: the browser no longer holds this response"));
    }

    #[test]
    fn a_network_entry_round_trips_with_its_absent_fields_off_the_wire() {
        let record = entry("1", "GET", "https://example.com/", Some(200));
        let encoded = serde_json::to_string(&record).expect("serialize");
        assert!(!encoded.contains("failure"));
        assert!(!encoded.contains("mime_type"));
        let decoded: NetworkEntry = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, record);
    }

    #[test]
    fn a_session_cookie_says_session_where_an_expiry_would_go() {
        let cookie = BrowserCookie {
            name: "sid".to_string(),
            value: "abc".to_string(),
            domain: "example.com".to_string(),
            path: "/".to_string(),
            secure: true,
            http_only: true,
            same_site: Some("Lax".to_string()),
            expires: None,
        };
        let line = render_cookie_line(&cookie);
        assert!(line.starts_with("sid=abc  example.com/"));
        assert!(line.contains("secure"));
        assert!(line.contains("httponly"));
        assert!(line.contains("samesite=Lax"));
        assert!(line.ends_with("session"));

        let persistent = BrowserCookie {
            expires: Some(1_800_000_000.0),
            ..cookie
        };
        assert!(render_cookie_line(&persistent).contains("expires=1800000000"));
    }

    #[test]
    fn a_storage_area_round_trips_through_its_cli_names_and_accessors() {
        for area in [StorageArea::Local, StorageArea::Session] {
            assert_eq!(StorageArea::parse(area.as_str()), Some(area));
            assert_eq!(StorageArea::parse(area.accessor()), Some(area));
        }
        assert_eq!(StorageArea::parse("cookies"), None);
        assert_eq!(StorageArea::Local.accessor(), "localStorage");
        assert_eq!(StorageArea::Session.accessor(), "sessionStorage");
    }

    #[test]
    fn an_empty_storage_area_says_so_rather_than_printing_nothing() {
        let snapshot = StorageSnapshot {
            area: StorageArea::Local,
            origin: "https://example.com".to_string(),
            entries: Vec::new(),
            truncated: false,
        };
        assert_eq!(
            render_storage(&snapshot),
            "localStorage is empty for https://example.com"
        );
    }

    #[test]
    fn a_truncated_storage_listing_reports_both_kinds_of_truncation() {
        let snapshot = StorageSnapshot {
            area: StorageArea::Session,
            origin: "https://example.com".to_string(),
            entries: vec![StorageEntry {
                key: "token".to_string(),
                value: "abc".to_string(),
                truncated: true,
            }],
            truncated: true,
        };
        let rendered = render_storage(&snapshot);
        assert!(rendered.contains("token=abc…"));
        assert!(rendered.contains(&format!("truncated at {MAX_STORAGE_BYTES} bytes")));
    }

    #[test]
    fn a_download_renders_its_resolved_path_once_it_has_one() {
        let mut record = DownloadRecord {
            guid: "guid-1".to_string(),
            url: "https://example.com/report.csv".to_string(),
            suggested_filename: "report.csv".to_string(),
            state: "in_progress".to_string(),
            received_bytes: 512,
            total_bytes: 2048,
            path: None,
        };
        let line = render_download_line(&record);
        assert!(line.contains("in_progress"));
        assert!(line.contains("512/2048"));
        assert!(line.contains("report.csv"));

        record.state = "completed".to_string();
        record.path = Some("/downloads/report.csv".to_string());
        assert!(render_download_line(&record).contains("/downloads/report.csv"));
    }

    #[test]
    fn a_truncated_snapshot_says_so_in_its_rendering() {
        let snapshot = PageSnapshot {
            generation: 1,
            url: String::new(),
            title: String::new(),
            interactive_only: false,
            truncated: true,
            elements: Vec::new(),
        };
        assert!(render_snapshot(&snapshot).contains("truncated"));
    }
}

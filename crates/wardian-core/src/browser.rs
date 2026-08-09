//! Wire types shared by the browser surface, the control plane, and the CLI.
//!
//! Runtime behavior lives in the app; this module holds only the shapes both
//! sides must agree on, so `wardian browser` and the workbench surface can
//! never drift apart.

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
    format!(
        "{}  {}  {}  {}{owner}{errors}",
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
        };
        let line = render_session_line(&summary);
        assert!(line.starts_with("browser:2  complete  https://example.com/  Example"));
        assert!(line.contains("agent=agent-1"));
        assert!(line.contains("console_errors=3"));
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
        };
        let line = render_session_line(&summary);
        assert!(line.contains("about:blank"));
        assert!(line.contains("(untitled)"));
        assert!(!line.contains("agent="));
        assert!(!line.contains("console_errors"));
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

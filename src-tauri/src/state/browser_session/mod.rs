//! Out-of-process browser sessions backing the workbench browser surface.
//!
//! See `docs/specs/2026-08-09-agent-browser-surface.md` for why the engine is
//! an external Chromium spoken to over CDP rather than a Tauri child webview.

mod actor;
mod cdp;
mod engine;
mod snapshot;

#[cfg(test)]
mod tests;

pub use actor::{
    normalize_url, BrowserError, BrowserSession, BrowserSessionBroker, BrowserSessionEvent,
    BrowserSessionSummary, ConsoleEntry, ElementAction, LoadState, OpenBrowserRequest, PageField,
    PointerEvent, Viewport, WaitCondition, DEFAULT_VIEWPORT_HEIGHT, DEFAULT_VIEWPORT_WIDTH,
};
pub use cdp::{CdpError, CdpEvent};
pub use engine::{discover_engine, EngineBinary, EngineError, EngineKind, ENGINE_BINARY_ENV};
pub use snapshot::{
    render_snapshot, PageSnapshot, RefError, SnapshotElement, MAX_SNAPSHOT_ELEMENTS,
};

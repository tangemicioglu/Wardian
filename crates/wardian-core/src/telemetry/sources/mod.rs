//! Provider source implementations.
//!
//! A source is anything that can be advanced from a cursor and asked for facts.
//! Two kinds exist and they are not interchangeable: codex, claude, and pi are
//! append-only JSONL files advanced by byte offset, while opencode is a live
//! SQLite database advanced by row timestamp. The trait is therefore "read
//! everything after this cursor", not "parse this string".

pub mod archive;
pub mod claude;
pub mod codex;
pub mod opencode;
pub mod pi;

use crate::telemetry::models::{Cursor, CursorKind, ParsedFacts, SourceCarry, SourceKind};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("source is unavailable: {0}")]
    Unavailable(String),
    /// The source exists but cannot be read right now, typically a locked
    /// database. Retried next cycle rather than waited on.
    #[error("source is busy: {0}")]
    Busy(String),
    #[error("source read failed: {0}")]
    Read(String),
}

/// Identity and location of the source being advanced.
#[derive(Debug, Clone)]
pub struct SourceContext {
    pub session_id: String,
    pub provider: String,
    pub path: PathBuf,
    /// Provider-native session identifiers, used by database sources to select
    /// the rows belonging to this agent.
    ///
    /// A list rather than one id because an agent accumulates a new provider
    /// session on every restart while continuing to write into the same shared
    /// database. Selecting on only the live one reports an agent's newest
    /// conversation as the whole of its history.
    pub provider_session_ids: Vec<String>,
}

impl SourceContext {
    pub fn new(session_id: impl Into<String>, provider: impl Into<String>, path: &Path) -> Self {
        Self {
            session_id: session_id.into(),
            provider: provider.into(),
            path: path.to_path_buf(),
            provider_session_ids: Vec::new(),
        }
    }

    pub fn with_provider_session_id(mut self, id: Option<String>) -> Self {
        self.provider_session_ids = id.into_iter().collect();
        self
    }

    pub fn with_provider_session_ids(mut self, ids: Vec<String>) -> Self {
        self.provider_session_ids = ids;
        self
    }

    /// The session recorded against this source in the store.
    ///
    /// A file-backed source has none, and a database source has many; this is
    /// the representative one kept for diagnostics, never for selection.
    pub fn primary_session_id(&self) -> Option<String> {
        self.provider_session_ids.first().cloned()
    }
}

pub trait TelemetrySource: Send + Sync {
    fn provider(&self) -> &'static str;

    /// Bumping this re-reads affected sources from the start, which is how a
    /// parser fix recovers facts it previously got wrong.
    fn parser_version(&self) -> i64;

    fn source_kind(&self) -> SourceKind;

    fn cursor_kind(&self) -> CursorKind;

    /// Read everything after `cursor`, returning facts and the new cursor.
    ///
    /// `carry` is the parser state left by the previous delta. Passing it in is
    /// what makes the result independent of where deltas were cut: without it, a
    /// record's attribution would depend on whether the ingest cycle happened to
    /// run between it and the context record it inherits from. Implementations
    /// return their end state in [`ParsedFacts::carry`], and never write to the
    /// telemetry store.
    fn read_since(
        &self,
        ctx: &SourceContext,
        cursor: Cursor,
        carry: SourceCarry,
    ) -> Result<(ParsedFacts, Cursor), SourceError>;

    /// Whether the stored cursor for this source is still valid.
    ///
    /// File-backed sources invalidate on replacement; a database source
    /// invalidates when the set of sessions its single cursor covers changes.
    fn cursor_is_stale(&self, _ctx: &SourceContext, _stored_fingerprint: Option<&str>) -> bool {
        false
    }

    /// What [`Self::cursor_is_stale`] compares against on the next pass.
    ///
    /// Defaults to the file's identity, which is what a byte cursor is measured
    /// against. A source whose cursor means something else must say so here, or
    /// its staleness check has nothing to compare with.
    fn fingerprint(&self, ctx: &SourceContext) -> Option<String> {
        crate::telemetry::identity::file_fingerprint(&ctx.path)
    }
}

/// Resolve a provider name to its source implementation.
///
/// The four native readers are not variations on one format. Codex, claude, and
/// pi are all append-only JSONL advanced by byte offset but disagree on what
/// `input_tokens` counts — codex's prompt total includes cache reads and is
/// corrected at ingest, while claude's and pi's already exclude them and are
/// stored raw. Opencode is a live database advanced by timestamp. Normalizing
/// those differences here, at ingest, is what lets a stored column mean one
/// thing regardless of which provider filled it.
///
/// Everything else falls back to [`archive::ArchiveSource`], which reads
/// Wardian's own conversation archive. That is how antigravity is covered: it
/// publishes no token accounting at all (corroborated by ccusage, which lists it
/// unsupported for that reason) and no parseable transcript, but Wardian watched
/// its turns happen and recorded their timestamps and the files they wrote.
/// Narrower telemetry is not the same as none, and the missing measures are
/// stored as unreported rather than as zero.
///
/// The fallback is last, never first: a provider with a native reader is never
/// also read through the archive, which would double count it.
pub fn source_for(provider: &str) -> Option<Box<dyn TelemetrySource>> {
    match provider {
        "codex" => Some(Box::new(codex::CodexSource)),
        "claude" => Some(Box::new(claude::ClaudeSource)),
        "opencode" => Some(Box::new(opencode::OpenCodeSource)),
        "pi" => Some(Box::new(pi::PiSource)),
        _ if uses_archive(provider) => Some(Box::new(archive::ArchiveSource)),
        _ => None,
    }
}

/// Whether a provider is read through the conversation archive.
///
/// An allow-list rather than a catch-all. The archive holds conversations for
/// anything Wardian has ever run, including mock agents and providers whose
/// records exist only as lifecycle noise, and admitting those would put test
/// fixtures in a habitat's history.
pub fn uses_archive(provider: &str) -> bool {
    matches!(provider, "antigravity" | "gemini")
}

/// Whether telemetry ingest supports a provider at all.
pub fn is_supported(provider: &str) -> bool {
    source_for(provider).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_providers_resolve() {
        assert!(is_supported("codex"));
        assert!(is_supported("claude"));
        assert!(is_supported("opencode"));
        assert!(is_supported("pi"));
    }

    #[test]
    fn providers_without_a_native_reader_fall_back_to_the_archive() {
        // Neither publishes telemetry anything can parse, which is why no parser
        // was written for them. That is a reason to read them differently, not a
        // reason to report their agents as having done nothing.
        assert!(is_supported("antigravity"));
        assert!(is_supported("gemini"));
        assert_eq!(source_for("antigravity").unwrap().provider(), "archive");
    }

    #[test]
    fn a_provider_with_a_native_reader_never_falls_back() {
        // Reading one agent through both its own log and the archive would count
        // every turn twice.
        for provider in ["codex", "claude", "opencode", "pi"] {
            assert!(!uses_archive(provider));
            assert_ne!(source_for(provider).unwrap().provider(), "archive");
        }
    }

    #[test]
    fn the_archive_fallback_is_an_allow_list_not_a_catch_all() {
        // The archive holds conversations for anything Wardian ever ran,
        // including mock agents used by the test suite.
        assert!(!is_supported("mock"));
        assert!(!is_supported("unknown"));
    }

    #[test]
    fn source_cursor_kinds_match_their_medium() {
        assert_eq!(
            source_for("codex").unwrap().cursor_kind(),
            CursorKind::ByteOffset
        );
        assert_eq!(
            source_for("opencode").unwrap().cursor_kind(),
            CursorKind::EpochMs
        );
    }
}

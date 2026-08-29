//! OpenCode source.
//!
//! Unlike every other provider, opencode keeps its state in a live SQLite
//! database (`~/.local/share/opencode/opencode.db`) rather than an append-only
//! log, so it is advanced by row timestamp rather than by byte offset.
//!
//! It is also the richest source and needs almost no parsing. Assistant rows in
//! `message` carry a five-way token split, a real `cost`, the model, and both
//! `time.created` and `time.completed` — meaning opencode reports **measured**
//! durations and must never be routed through the gap clusterer. That makes it
//! the reference implementation the inferring providers are validated against.
//!
//! Access is strictly read-only. `immutable=1` is deliberately not used: it
//! would skip WAL contents and silently under-report exactly the recent
//! activity the Dashboard is about.

use crate::telemetry::activity::format_epoch_ms;
use crate::telemetry::models::{
    ActivityMethod, Cursor, CursorKind, EditFact, EditOp, IntervalFact, ParsedFacts, SourceCarry,
    SourceKind, TurnFact,
};
use crate::telemetry::sources::{SourceContext, SourceError, TelemetrySource};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

/// Rows are re-read this far behind the high-water mark, because a concurrent
/// writer can commit a row whose `time_updated` is below a mark we already
/// advanced past. Re-reading is safe: every fact table carries a uniqueness
/// constraint that makes a repeated row a no-op rather than a double count.
const CURSOR_LAG_MS: i64 = 5_000;

#[derive(Debug, Default)]
pub struct OpenCodeSource;

impl TelemetrySource for OpenCodeSource {
    fn provider(&self) -> &'static str {
        "opencode"
    }

    fn parser_version(&self) -> i64 {
        1
    }

    fn source_kind(&self) -> SourceKind {
        SourceKind::Sqlite
    }

    fn cursor_kind(&self) -> CursorKind {
        CursorKind::EpochMs
    }

    /// `carry` is unused: every opencode row states its own identity, so nothing
    /// has to be inherited across a delta boundary.
    fn read_since(
        &self,
        ctx: &SourceContext,
        cursor: Cursor,
        _carry: SourceCarry,
    ) -> Result<(ParsedFacts, Cursor), SourceError> {
        let conn = open_read_only(&ctx.path)?;
        read_since_conn(&conn, ctx, cursor)
    }

    /// A cursor over a shared database only means something for the exact set of
    /// sessions it was advanced across.
    ///
    /// The store keys this source by agent and database path, so a session
    /// discovered after the fact would otherwise inherit a cursor that is
    /// already past all of its rows and never be read. Comparing the set's
    /// fingerprint turns that into a re-read, which uniqueness makes free.
    fn cursor_is_stale(&self, ctx: &SourceContext, stored_fingerprint: Option<&str>) -> bool {
        match stored_fingerprint {
            Some(stored) => stored != session_set_fingerprint(&ctx.provider_session_ids),
            // Never fingerprinted: state predating this check. Re-reading every
            // opencode source once is cheap; assuming it is current is not.
            None => true,
        }
    }

    fn fingerprint(&self, ctx: &SourceContext) -> Option<String> {
        Some(session_set_fingerprint(&ctx.provider_session_ids))
    }
}

fn open_read_only(path: &std::path::Path) -> Result<Connection, SourceError> {
    if !path.exists() {
        return Err(SourceError::Unavailable(format!(
            "opencode database not found at {}",
            path.display()
        )));
    }
    // READ_ONLY without immutable: we must see WAL-resident rows.
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(map_sqlite_error)
}

/// Identity of the session set this source is currently reading.
///
/// One timestamp cursor covers every session an agent owns in the shared
/// database, which is only sound while that set is fixed. When a session is
/// discovered *later* — recovered from the archive, or newly attributed by
/// workspace — all of its rows sit below a cursor another session already
/// advanced, and it would be skipped forever. Recording the set's identity lets
/// a change invalidate the cursor and force a re-read, which the `event_key`
/// uniqueness constraints make idempotent.
pub fn session_set_fingerprint(session_ids: &[String]) -> String {
    let mut sorted: Vec<&str> = session_ids.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    crate::telemetry::identity::content_key(sorted.join("\u{1f}").as_bytes())
}

/// Every session opencode recorded against a workspace directory.
///
/// The counterpart of the projected-home scan the file-backed providers get.
/// An agent's *recorded* session ids only cover conversations Wardian happened
/// to archive, so an agent that has been restarted many times, or run headless,
/// has sessions no id list knows about. Opencode stamps each session with the
/// directory it ran in, which attributes those the same way a per-agent habitat
/// attributes a rollout file.
///
/// Matching is lexical after normalizing separators and case, because opencode
/// writes `D:/Development/x` where Wardian holds `D:\Development\x`, and Windows
/// paths are case-insensitive. No canonicalization: the directory may no longer
/// exist, and a session that ran there is still that agent's history.
pub fn sessions_in_directory(
    path: &std::path::Path,
    directory: &str,
) -> Result<Vec<String>, SourceError> {
    let conn = open_read_only(path)?;
    let wanted = normalize_directory(directory);
    if wanted.is_empty() {
        return Ok(Vec::new());
    }

    let mut stmt = conn
        .prepare("SELECT id, directory FROM session WHERE directory IS NOT NULL")
        .map_err(map_sqlite_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(map_sqlite_error)?;

    let mut found = Vec::new();
    for row in rows {
        let (id, directory) = row.map_err(map_sqlite_error)?;
        if normalize_directory(&directory) == wanted {
            found.push(id);
        }
    }
    Ok(found)
}

fn normalize_directory(directory: &str) -> String {
    directory
        .trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

fn map_sqlite_error(err: rusqlite::Error) -> SourceError {
    let text = err.to_string();
    if text.contains("locked") || text.contains("busy") {
        SourceError::Busy(text)
    } else {
        SourceError::Read(text)
    }
}

pub(crate) fn read_since_conn(
    conn: &Connection,
    ctx: &SourceContext,
    cursor: Cursor,
) -> Result<(ParsedFacts, Cursor), SourceError> {
    if ctx.provider_session_ids.is_empty() {
        return Err(SourceError::Unavailable(
            "opencode source requires a provider session id".to_string(),
        ));
    }

    let since = cursor.value.saturating_sub(CURSOR_LAG_MS).max(0);
    let mut facts = ParsedFacts::default();
    // The cursor is one position over a database holding every session this
    // agent has ever run, so it can only advance as far as the furthest session
    // has been consumed. Taking the maximum rather than the minimum is what
    // stops a long-finished session from pinning the cursor in its own past and
    // making every later pass re-read the active one from there.
    let mut next = cursor.value;

    for session in &ctx.provider_session_ids {
        read_messages(conn, ctx, session, since, &mut facts)?;
        read_parts(conn, ctx, session, since, &mut facts)?;
        next = next.max(next_cursor_value(conn, session, cursor.value)?);
    }

    Ok((facts, Cursor::new(CursorKind::EpochMs, next)))
}

/// The point below which every table has been fully consumed.
///
/// `message` and `part` advance on independent timelines — a turn's parts are
/// written while it runs, its message row when it finishes — so a single
/// high-water mark taken across both can be dragged past unread rows of the
/// slower table by the faster one. Taking the minimum makes the cursor mean
/// "seen everything up to here, everywhere", and the rows above it are simply
/// re-read next pass, which the uniqueness constraints make free.
///
/// A table holding nothing for this session imposes no constraint; otherwise a
/// session that never produced parts could never advance at all.
fn next_cursor_value(conn: &Connection, session: &str, current: i64) -> Result<i64, SourceError> {
    let mut bound: Option<i64> = None;
    for table in ["message", "part"] {
        let max: Option<i64> = conn
            .query_row(
                &format!("SELECT MAX(time_updated) FROM {table} WHERE session_id = ?1"),
                rusqlite::params![session],
                |row| row.get(0),
            )
            .map_err(map_sqlite_error)?;
        if let Some(max) = max {
            bound = Some(bound.map_or(max, |seen: i64| seen.min(max)));
        }
    }
    // Both maxima only grow in normal operation, so this advances. If a row is
    // deleted underneath us the bound can drop, and re-reading is the right
    // response to that anyway.
    Ok(bound.unwrap_or(current))
}

fn read_messages(
    conn: &Connection,
    ctx: &SourceContext,
    session: &str,
    since: i64,
    facts: &mut ParsedFacts,
) -> Result<(), SourceError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, time_updated, data
             FROM message
             WHERE session_id = ?1 AND time_updated > ?2
             ORDER BY time_updated",
        )
        .map_err(map_sqlite_error)?;

    let rows = stmt
        .query_map(rusqlite::params![session, since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(map_sqlite_error)?;

    for row in rows {
        let (message_id, _time_updated, data) = row.map_err(map_sqlite_error)?;

        let Ok(message) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let time = message.get("time");
        let created = time.and_then(|t| t.get("created")).and_then(Value::as_i64);
        // An in-flight message has no completion time. Skipping it is safe:
        // finishing bumps `time_updated`, so it returns on a later pass.
        let Some(completed) = time
            .and_then(|t| t.get("completed"))
            .and_then(Value::as_i64)
        else {
            continue;
        };
        let Some(ended_at) = format_epoch_ms(completed) else {
            continue;
        };
        let started_at = created.and_then(format_epoch_ms);

        let tokens = message.get("tokens");
        let cache = tokens.and_then(|t| t.get("cache"));
        facts.turns.push(TurnFact {
            // The provider's own row id: already stable and unique, so nothing
            // has to be derived.
            event_key: message_id.clone(),
            session_id: ctx.session_id.clone(),
            provider: "opencode".to_string(),
            turn_id: Some(message_id.clone()),
            model: message
                .get("modelID")
                .and_then(Value::as_str)
                .map(str::to_string),
            effort: None,
            started_at: started_at.clone(),
            ended_at: ended_at.clone(),
            input_tokens: tokens.and_then(|t| t.get("input")).and_then(Value::as_i64),
            cached_input_tokens: cache.and_then(|c| c.get("read")).and_then(Value::as_i64),
            cache_write_tokens: cache.and_then(|c| c.get("write")).and_then(Value::as_i64),
            output_tokens: tokens.and_then(|t| t.get("output")).and_then(Value::as_i64),
            reasoning_tokens: tokens
                .and_then(|t| t.get("reasoning"))
                .and_then(Value::as_i64),
            context_window: None,
            // Captured because opencode hands it over for free; not surfaced,
            // since no other provider reports it.
            cost_usd: message.get("cost").and_then(Value::as_f64),
        });

        // Real start and end: a measured interval, never a clustered one.
        if let (Some(started_at), true) = (started_at, completed >= created.unwrap_or(completed)) {
            facts.intervals.push(IntervalFact {
                session_id: ctx.session_id.clone(),
                provider: "opencode".to_string(),
                started_at,
                // A measured span carries no singleton credit, so its end is
                // already the last real moment of the turn.
                last_event_at: ended_at.clone(),
                ended_at,
                event_count: 1,
                method: ActivityMethod::Measured,
            });
        }
    }

    Ok(())
}

fn read_parts(
    conn: &Connection,
    ctx: &SourceContext,
    session: &str,
    since: i64,
    facts: &mut ParsedFacts,
) -> Result<(), SourceError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, message_id, time_updated, data
             FROM part
             WHERE session_id = ?1 AND time_updated > ?2
             ORDER BY time_updated",
        )
        .map_err(map_sqlite_error)?;

    let rows = stmt
        .query_map(rusqlite::params![session, since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(map_sqlite_error)?;

    for row in rows {
        let (part_id, message_id, time_updated, data) = row.map_err(map_sqlite_error)?;

        let Ok(part) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if part.get("type").and_then(Value::as_str) != Some("tool") {
            continue;
        }
        let state = part.get("state");
        if state.and_then(|s| s.get("status")).and_then(Value::as_str) != Some("completed") {
            continue;
        }
        let Some(input) = state.and_then(|s| s.get("input")) else {
            continue;
        };
        let Some(occurred_at) = format_epoch_ms(time_updated) else {
            continue;
        };

        let tool = part.get("tool").and_then(Value::as_str).unwrap_or("");
        match tool {
            "edit" => {
                if let Some(path) = input.get("filePath").and_then(Value::as_str) {
                    let (added, removed) = changed_line_counts(
                        input.get("oldString").and_then(Value::as_str).unwrap_or(""),
                        input.get("newString").and_then(Value::as_str).unwrap_or(""),
                    );
                    facts.edits.push(EditFact {
                        event_key: part_id.clone(),
                        session_id: ctx.session_id.clone(),
                        provider: "opencode".to_string(),
                        turn_id: message_id.clone(),
                        occurred_at,
                        workspace: None,
                        path: path.to_string(),
                        op: EditOp::Update,
                        lines_added: Some(added),
                        lines_removed: Some(removed),
                    });
                }
            }
            "write" => {
                if let Some(path) = input.get("filePath").and_then(Value::as_str) {
                    facts.edits.push(EditFact {
                        event_key: part_id.clone(),
                        session_id: ctx.session_id.clone(),
                        provider: "opencode".to_string(),
                        turn_id: message_id.clone(),
                        occurred_at,
                        workspace: None,
                        path: path.to_string(),
                        op: EditOp::Add,
                        lines_added: Some(count_lines(
                            input.get("content").and_then(Value::as_str).unwrap_or(""),
                        )),
                        lines_removed: Some(0),
                    });
                }
            }
            "apply_patch" => {
                if let Some(text) = input.get("patchText").and_then(Value::as_str) {
                    for edit in
                        parse_patch_text(ctx, text, &occurred_at, &part_id, message_id.clone())
                    {
                        facts.edits.push(edit);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Parse opencode's `apply_patch` envelope.
///
/// ```text
/// *** Begin Patch
/// *** Update File: <path>
/// @@
/// -removed
/// +added
/// *** End Patch
/// ```
fn parse_patch_text(
    ctx: &SourceContext,
    text: &str,
    occurred_at: &str,
    part_id: &str,
    turn_id: Option<String>,
) -> Vec<EditFact> {
    let mut edits: Vec<EditFact> = Vec::new();
    let mut current: Option<(String, EditOp, i64, i64)> = None;

    let flush = |current: Option<(String, EditOp, i64, i64)>, edits: &mut Vec<EditFact>| {
        if let Some((path, op, added, removed)) = current {
            edits.push(EditFact {
                // One patch can touch several files; the path separates them.
                event_key: format!("{part_id}:{path}"),
                session_id: ctx.session_id.clone(),
                provider: "opencode".to_string(),
                turn_id: turn_id.clone(),
                occurred_at: occurred_at.to_string(),
                workspace: None,
                path,
                op,
                lines_added: Some(added),
                lines_removed: Some(removed),
            });
        }
    };

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("*** ") {
            if let Some((op, path)) = parse_file_header(rest) {
                flush(current.take(), &mut edits);
                current = Some((path, op, 0, 0));
            } else if rest.starts_with("End Patch") {
                flush(current.take(), &mut edits);
            }
            continue;
        }
        if let Some((_, _, added, removed)) = current.as_mut() {
            match line.chars().next() {
                Some('+') => *added += 1,
                Some('-') => *removed += 1,
                _ => {}
            }
        }
    }
    flush(current.take(), &mut edits);
    edits
}

fn parse_file_header(rest: &str) -> Option<(EditOp, String)> {
    for (prefix, op) in [
        ("Add File: ", EditOp::Add),
        ("Update File: ", EditOp::Update),
        ("Delete File: ", EditOp::Delete),
    ] {
        if let Some(path) = rest.strip_prefix(prefix) {
            return Some((op, path.trim().to_string()));
        }
    }
    None
}

fn count_lines(content: &str) -> i64 {
    if content.is_empty() {
        return 0;
    }
    content.lines().count() as i64
}

/// Lines actually changed between two versions of a block.
///
/// The `edit` tool hands over whole replaced blocks rather than a diff, so
/// counting them raw would charge every unchanged line of context as both an
/// addition and a deletion. Trimming the common prefix and suffix reduces a
/// contiguous edit to the same figures a unified diff would report, keeping
/// opencode's line counts comparable with codex's.
fn changed_line_counts(old: &str, new: &str) -> (i64, i64) {
    let old_lines: Vec<&str> = if old.is_empty() {
        Vec::new()
    } else {
        old.lines().collect()
    };
    let new_lines: Vec<&str> = if new.is_empty() {
        Vec::new()
    } else {
        new.lines().collect()
    };

    let mut prefix = 0;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0;
    while suffix < old_lines.len() - prefix
        && suffix < new_lines.len() - prefix
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let removed = (old_lines.len() - prefix - suffix) as i64;
    let added = (new_lines.len() - prefix - suffix) as i64;
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> SourceContext {
        SourceContext::new("agent-1", "opencode", std::path::Path::new("opencode.db"))
            .with_provider_session_id(Some("ses_test".to_string()))
    }

    /// A real file on disk holding only the `session` columns attribution reads.
    ///
    /// `sessions_in_directory` opens by path rather than taking a connection,
    /// because it runs during discovery where no connection exists yet.
    fn seed_session_file() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT);
             INSERT INTO session VALUES ('ses_a', 'D:/Development/Wardian');
             INSERT INTO session VALUES ('ses_b', 'D:\\Development\\Wardian');
             INSERT INTO session VALUES ('ses_c', 'D:/Development/Wardian/');
             INSERT INTO session VALUES ('ses_d', 'd:/development/WARDIAN');
             INSERT INTO session VALUES ('ses_e', 'D:/Development/Other');
             INSERT INTO session VALUES ('ses_f', NULL);",
        )
        .unwrap();
        drop(conn);
        (dir, path)
    }

    #[test]
    fn sessions_are_attributed_by_the_directory_they_ran_in() {
        // An agent's recorded session ids only cover conversations Wardian
        // archived. Without this, an agent restarted many times reports only the
        // handful of sessions that happened to be captured.
        let (_dir, path) = seed_session_file();
        let mut ids = sessions_in_directory(&path, "D:/Development/Wardian").unwrap();
        ids.sort();
        assert_eq!(ids, vec!["ses_a", "ses_b", "ses_c", "ses_d"]);
    }

    #[test]
    fn attribution_survives_separator_case_and_trailing_slash() {
        // Opencode writes `D:/Development/x` where Wardian holds
        // `D:\Development\x`, and Windows paths are case-insensitive. Comparing
        // raw strings would attribute nothing on the platform this runs on.
        let (_dir, path) = seed_session_file();
        for variant in [
            "D:\\Development\\Wardian",
            "d:/development/wardian",
            "D:/Development/Wardian/",
            "  D:/Development/Wardian  ",
        ] {
            assert_eq!(
                sessions_in_directory(&path, variant).unwrap().len(),
                4,
                "variant {variant} did not attribute"
            );
        }
    }

    #[test]
    fn a_different_directory_is_not_this_agents_work() {
        let (_dir, path) = seed_session_file();
        assert!(sessions_in_directory(&path, "D:/Development/Other")
            .unwrap()
            .contains(&"ses_e".to_string()));
        assert!(sessions_in_directory(&path, "D:/Development/Nothing")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn an_empty_workspace_attributes_nothing_rather_than_everything() {
        // A blank directory must not match the whole database.
        let (_dir, path) = seed_session_file();
        assert!(sessions_in_directory(&path, "   ").unwrap().is_empty());
    }

    #[test]
    fn a_missing_database_is_unavailable_not_a_panic() {
        let err = sessions_in_directory(std::path::Path::new("no-such.db"), "D:/x").unwrap_err();
        assert!(matches!(err, SourceError::Unavailable(_)));
    }

    /// Mirrors the live opencode schema for the columns this source reads.
    fn seed_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER,
                                   time_updated INTEGER, data TEXT);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT,
                                time_created INTEGER, time_updated INTEGER, data TEXT);",
        )
        .unwrap();
        conn
    }

    fn insert_message(conn: &Connection, id: &str, updated: i64, data: &str) {
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, 'ses_test', ?2, ?2, ?3)",
            rusqlite::params![id, updated, data],
        )
        .unwrap();
    }

    fn insert_part(conn: &Connection, id: &str, updated: i64, data: &str) {
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES (?1, 'msg_1', 'ses_test', ?2, ?2, ?3)",
            rusqlite::params![id, updated, data],
        )
        .unwrap();
    }

    // Verbatim shape captured from the live opencode database on 2026-08-13.
    const ASSISTANT: &str = r#"{"role":"assistant","mode":"build","agent":"build",
        "path":{"cwd":"D:\\repo","root":"D:\\repo"},"cost":0.0125,
        "tokens":{"total":12499,"input":10639,"output":68,"reasoning":0,
                  "cache":{"write":0,"read":1792}},
        "modelID":"deepseek-v4-flash-free","providerID":"opencode",
        "time":{"created":1786644338919,"completed":1786644345306},"finish":"stop"}"#;

    #[test]
    fn assistant_message_becomes_a_turn_with_split_tokens() {
        let conn = seed_db();
        insert_message(&conn, "msg_1", 1786644345306, ASSISTANT);
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();

        assert_eq!(facts.turns.len(), 1);
        let turn = &facts.turns[0];
        assert_eq!(turn.input_tokens, Some(10639));
        assert_eq!(turn.cached_input_tokens, Some(1792));
        assert_eq!(turn.cache_write_tokens, Some(0));
        assert_eq!(turn.output_tokens, Some(68));
        assert_eq!(turn.model.as_deref(), Some("deepseek-v4-flash-free"));
        assert_eq!(turn.cost_usd, Some(0.0125));
    }

    #[test]
    fn opencodes_input_is_already_cache_exclusive_so_it_is_stored_unadjusted() {
        // The asymmetry that makes `input_tokens` a normalized field rather
        // than a passthrough one. Codex reports a prompt total that *includes*
        // cache reads and its source subtracts them; opencode reports the
        // components disjointly, which its own `total` proves:
        //
        //     total 12499 = input 10639 + output 68 + read 1792 + write 0
        //
        // Subtracting here too would delete real usage. Asserted rather than
        // assumed, because the two providers agreeing is the assumption that
        // caused a 49x overstatement the first time round.
        let conn = seed_db();
        insert_message(&conn, "msg_1", 1786644345306, ASSISTANT);
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        let turn = &facts.turns[0];

        let declared_total: i64 = serde_json::from_str::<serde_json::Value>(ASSISTANT).unwrap()
            ["tokens"]["total"]
            .as_i64()
            .unwrap();
        assert_eq!(
            turn.input_tokens.unwrap()
                + turn.output_tokens.unwrap()
                + turn.cached_input_tokens.unwrap()
                + turn.cache_write_tokens.unwrap(),
            declared_total
        );
        // Stored exactly as reported: no subtraction happened.
        assert_eq!(turn.input_tokens, Some(10639));
    }

    #[test]
    fn opencode_intervals_are_measured_not_clustered() {
        let conn = seed_db();
        insert_message(&conn, "msg_1", 1786644345306, ASSISTANT);
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();

        assert_eq!(facts.intervals.len(), 1);
        assert_eq!(facts.intervals[0].method, ActivityMethod::Measured);
        // Never feed the clusterer; that would replace a fact with an estimate.
        assert!(facts.event_times.is_empty());
    }

    #[test]
    fn interval_spans_the_real_turn_duration() {
        let conn = seed_db();
        insert_message(&conn, "msg_1", 1786644345306, ASSISTANT);
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        let duration = crate::telemetry::activity::interval_duration_ms(&facts.intervals[0]);
        assert_eq!(duration, 1786644345306 - 1786644338919);
    }

    #[test]
    fn user_messages_are_not_turns() {
        let conn = seed_db();
        insert_message(
            &conn,
            "msg_1",
            100,
            r#"{"role":"user","time":{"created":90}}"#,
        );
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        assert!(facts.turns.is_empty());
    }

    #[test]
    fn in_flight_message_is_skipped_until_it_completes() {
        let conn = seed_db();
        insert_message(
            &conn,
            "msg_1",
            100,
            r#"{"role":"assistant","time":{"created":90}}"#,
        );
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        assert!(facts.turns.is_empty());
    }

    #[test]
    fn cursor_advances_to_the_high_water_mark() {
        let conn = seed_db();
        insert_message(&conn, "msg_1", 1786644345306, ASSISTANT);
        let (_, cursor) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        assert_eq!(cursor.kind, CursorKind::EpochMs);
        assert_eq!(cursor.value, 1786644345306);
    }

    #[test]
    fn other_sessions_are_not_read() {
        let conn = seed_db();
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES ('msg_other', 'ses_other', 1, 1, ?1)",
            rusqlite::params![ASSISTANT],
        )
        .unwrap();
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        assert!(facts.turns.is_empty());
    }

    #[test]
    fn missing_provider_session_id_is_an_error_not_a_full_scan() {
        let conn = seed_db();
        let ctx = SourceContext::new("agent-1", "opencode", std::path::Path::new("opencode.db"));
        let err = read_since_conn(&conn, &ctx, Cursor::start(CursorKind::EpochMs)).unwrap_err();
        assert!(matches!(err, SourceError::Unavailable(_)));
    }

    #[test]
    fn edit_tool_counts_both_sides() {
        let conn = seed_db();
        insert_part(
            &conn,
            "prt_1",
            1786644345306,
            r#"{"type":"tool","tool":"edit","state":{"status":"completed",
                "input":{"filePath":"D:/repo/a.md","oldString":"one\ntwo","newString":"1\n2\n3"}}}"#,
        );
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        assert_eq!(facts.edits.len(), 1);
        assert_eq!(facts.edits[0].op, EditOp::Update);
        assert_eq!(facts.edits[0].lines_added, Some(3));
        assert_eq!(facts.edits[0].lines_removed, Some(2));
    }

    #[test]
    fn unchanged_context_lines_are_not_charged_as_edits() {
        // The edit tool passes whole replaced blocks. Counting them raw would
        // report 4 added and 4 removed for a one-line change.
        let conn = seed_db();
        insert_part(
            &conn,
            "prt_1",
            1786644345306,
            r#"{"type":"tool","tool":"edit","state":{"status":"completed",
                "input":{"filePath":"a.md","oldString":"a\nb\nOLD\nd","newString":"a\nb\nNEW\nd"}}}"#,
        );
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        assert_eq!(facts.edits[0].lines_added, Some(1));
        assert_eq!(facts.edits[0].lines_removed, Some(1));
    }

    #[test]
    fn pure_insertion_removes_nothing() {
        assert_eq!(changed_line_counts("a\nb", "a\nnew\nb"), (1, 0));
    }

    #[test]
    fn pure_deletion_adds_nothing() {
        assert_eq!(changed_line_counts("a\ngone\nb", "a\nb"), (0, 1));
    }

    #[test]
    fn an_empty_old_block_is_all_additions() {
        assert_eq!(changed_line_counts("", "a\nb\nc"), (3, 0));
    }

    #[test]
    fn identical_blocks_change_nothing() {
        assert_eq!(changed_line_counts("a\nb", "a\nb"), (0, 0));
    }

    #[test]
    fn write_tool_counts_content_as_additions() {
        let conn = seed_db();
        insert_part(
            &conn,
            "prt_1",
            1786644345306,
            r#"{"type":"tool","tool":"write","state":{"status":"completed",
                "input":{"filePath":"D:/repo/new.md","content":"a\nb\nc\nd"}}}"#,
        );
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        assert_eq!(facts.edits[0].op, EditOp::Add);
        assert_eq!(facts.edits[0].lines_added, Some(4));
    }

    #[test]
    fn incomplete_tool_calls_are_ignored() {
        let conn = seed_db();
        insert_part(
            &conn,
            "prt_1",
            100,
            r#"{"type":"tool","tool":"edit","state":{"status":"running",
                "input":{"filePath":"a.md","oldString":"x","newString":"y"}}}"#,
        );
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        assert!(facts.edits.is_empty());
    }

    #[test]
    fn read_tool_is_not_an_edit() {
        let conn = seed_db();
        insert_part(
            &conn,
            "prt_1",
            100,
            r#"{"type":"tool","tool":"read","state":{"status":"completed","input":{"filePath":"a.md"}}}"#,
        );
        let (facts, _) =
            read_since_conn(&conn, &ctx(), Cursor::start(CursorKind::EpochMs)).unwrap();
        assert!(facts.edits.is_empty());
    }

    // --- apply_patch envelope ---

    #[test]
    fn patch_text_splits_per_file_with_line_counts() {
        let text = "*** Begin Patch\n\
                    *** Update File: D:\\repo\\a.tsx\n\
                    @@\n\
                    -old line\n\
                    +new line\n\
                    +another\n\
                    *** Add File: D:\\repo\\b.md\n\
                    +fresh\n\
                    *** End Patch\n";
        let edits = parse_patch_text(&ctx(), text, "2026-08-13T00:00:00.000Z", "prt_1", None);
        assert_eq!(edits.len(), 2);

        assert_eq!(edits[0].path, "D:\\repo\\a.tsx");
        assert_eq!(edits[0].op, EditOp::Update);
        assert_eq!(edits[0].lines_added, Some(2));
        assert_eq!(edits[0].lines_removed, Some(1));

        assert_eq!(edits[1].path, "D:\\repo\\b.md");
        assert_eq!(edits[1].op, EditOp::Add);
        assert_eq!(edits[1].lines_added, Some(1));
    }

    #[test]
    fn patch_text_handles_deletes() {
        let text = "*** Begin Patch\n*** Delete File: D:\\repo\\gone.md\n*** End Patch\n";
        let edits = parse_patch_text(&ctx(), text, "2026-08-13T00:00:00.000Z", "prt_1", None);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, EditOp::Delete);
    }

    #[test]
    fn patch_markers_are_not_counted_as_content_lines() {
        let text = "*** Begin Patch\n*** Update File: a.md\n@@\n+one\n*** End Patch\n";
        let edits = parse_patch_text(&ctx(), text, "2026-08-13T00:00:00.000Z", "prt_1", None);
        assert_eq!(edits[0].lines_added, Some(1));
        assert_eq!(edits[0].lines_removed, Some(0));
    }

    #[test]
    fn cursor_lag_re_reads_recent_rows() {
        let conn = seed_db();
        insert_message(&conn, "msg_1", 10_000, ASSISTANT);
        // A cursor just past the row still re-reads it, because a concurrent
        // writer may have committed below the mark. Idempotence is enforced by
        // the store's uniqueness constraints, not by the cursor.
        let (facts, _) = read_since_conn(
            &conn,
            &ctx(),
            Cursor::new(CursorKind::EpochMs, 10_000 + CURSOR_LAG_MS - 1),
        )
        .unwrap();
        assert_eq!(facts.turns.len(), 1);
    }
}

//! Fact persistence and cursor bookkeeping.
//!
//! Every write path here is idempotent by construction. Byte-offset provider
//! logs are stored as five-minute source-owned aggregates because no interface
//! reads individual callbacks. Timestamp-cursor sources retain their event
//! identities because their overlap reread needs them to avoid double counts.

use crate::telemetry::identity::compact_fact_key;
pub use crate::telemetry::identity::source_key;
use crate::telemetry::models::{
    ActivityMethod, Cursor, CursorKind, EditFact, IntervalFact, LimitObservation, ParsedFacts,
    SourceCarry, SourceKind, TurnFact,
};
use crate::telemetry::schema::{
    ensure_source_ref, ensure_string_id, STRING_EFFORT, STRING_MODEL, STRING_PROVIDER,
    STRING_SESSION,
};
use chrono::Timelike;
use rusqlite::{params, Connection, OptionalExtension};

/// Bookkeeping for one source's ingest position.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceState {
    /// Identity of this provider source, from [`source_key`].
    pub source_key: String,
    pub source_path: String,
    pub session_id: String,
    pub provider_session_id: Option<String>,
    pub provider: String,
    pub source_kind: SourceKind,
    pub cursor: Cursor,
    pub last_size: i64,
    pub last_modified: Option<String>,
    pub parser_version: i64,
    /// Identity of the bytes the cursor refers to; see [`file_fingerprint`].
    pub fingerprint: Option<String>,
    pub carry: SourceCarry,
}

/// The identity a cursor belongs to.
///
/// OpenCode keeps one database for every agent on the machine, so its agent
/// session remains part of the key. File-backed Codex and Claude sources are
/// already one source per physical path and intentionally omit that agent id.
pub fn load_source_state(
    conn: &Connection,
    source_key: &str,
) -> rusqlite::Result<Option<SourceState>> {
    conn.query_row(
        "SELECT source_key, source_path, session_id, provider_session_id, provider, source_kind,
                cursor_kind, cursor_value, last_size, last_modified, parser_version, fingerprint,
                carry_turn_id, carry_model, carry_effort, carry_cwd
         FROM telemetry_sources WHERE source_key = ?1",
        params![source_key],
        |row| {
            let cursor_kind =
                CursorKind::parse(&row.get::<_, String>(6)?).unwrap_or(CursorKind::ByteOffset);
            let source_kind = if row.get::<_, String>(5)? == "sqlite" {
                SourceKind::Sqlite
            } else {
                SourceKind::Jsonl
            };
            Ok(SourceState {
                source_key: row.get(0)?,
                source_path: row.get(1)?,
                session_id: row.get(2)?,
                provider_session_id: row.get(3)?,
                provider: row.get(4)?,
                source_kind,
                cursor: Cursor::new(cursor_kind, row.get(7)?),
                last_size: row.get(8)?,
                last_modified: row.get(9)?,
                parser_version: row.get(10)?,
                fingerprint: row.get(11)?,
                carry: SourceCarry {
                    turn_id: row.get(12)?,
                    model: row.get(13)?,
                    effort: row.get(14)?,
                    cwd: row.get(15)?,
                },
            })
        },
    )
    .optional()
}

pub fn save_source_state(conn: &Connection, state: &SourceState) -> rusqlite::Result<()> {
    save_source_state_on(conn, state)
}

const SAVE_SOURCE_STATE_SQL: &str = "INSERT INTO telemetry_sources (
     source_key, source_path, session_id, provider_session_id, provider, source_kind,
     cursor_kind, cursor_value, last_size, last_modified, last_ingested_at, parser_version,
     fingerprint, carry_turn_id, carry_model, carry_effort, carry_cwd
 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
 ON CONFLICT(source_key) DO UPDATE SET
     source_path = excluded.source_path,
     session_id = excluded.session_id,
     provider_session_id = excluded.provider_session_id,
     provider = excluded.provider,
     source_kind = excluded.source_kind,
     cursor_kind = excluded.cursor_kind,
     cursor_value = excluded.cursor_value,
     last_size = excluded.last_size,
     last_modified = excluded.last_modified,
     last_ingested_at = excluded.last_ingested_at,
     parser_version = excluded.parser_version,
     fingerprint = excluded.fingerprint,
     carry_turn_id = excluded.carry_turn_id,
     carry_model = excluded.carry_model,
     carry_effort = excluded.carry_effort,
     carry_cwd = excluded.carry_cwd";

fn save_source_state_on(conn: &Connection, state: &SourceState) -> rusqlite::Result<()> {
    conn.execute(
        SAVE_SOURCE_STATE_SQL,
        params![
            state.source_key,
            state.source_path,
            state.session_id,
            state.provider_session_id,
            state.provider,
            state.source_kind.as_str(),
            state.cursor.kind.as_str(),
            state.cursor.value,
            state.last_size,
            state.last_modified,
            now_rfc3339(),
            state.parser_version,
            state.fingerprint,
            state.carry.turn_id,
            state.carry.model,
            state.carry.effort,
            state.carry.cwd,
        ],
    )?;
    Ok(())
}

/// Distinct hour buckets a batch of facts touched, so rollups recompute only
/// what changed rather than the whole horizon.
pub type DirtyBuckets = std::collections::BTreeSet<(String, String)>;

/// Write facts and advance the cursor.
///
/// Takes `&Connection` rather than owning a transaction so the caller can put
/// this and the rollup rebuild inside one. Committing them separately would let
/// a crash in between leave the cursor past facts whose buckets were never
/// recomputed — and since the cursor never moves back, those buckets would stay
/// stale forever.
pub(crate) fn write_facts(
    conn: &Connection,
    facts: &ParsedFacts,
    intervals: &[IntervalFact],
    state: &SourceState,
) -> rusqlite::Result<DirtyBuckets> {
    // Byte-offset sources advance atomically with their facts, so a repeated
    // write at the same cursor is the only duplicate case left after facts are
    // aggregated. OpenCode and archive sources deliberately do not take this
    // shortcut: their timestamp cursors reread an overlap and still require
    // event-level uniqueness.
    if compact_source(state)
        && (!facts.turns.is_empty() || !facts.edits.is_empty())
        && load_source_state(conn, &state.source_key)?.is_some_and(|previous| {
            previous.cursor == state.cursor
                && previous.parser_version == state.parser_version
                && previous.fingerprint == state.fingerprint
        })
    {
        return Ok(DirtyBuckets::new());
    }

    let mut dirty = DirtyBuckets::new();
    let source_ref = ensure_source_ref(conn, &state.source_key, &state.source_path)?;

    for turn in &facts.turns {
        insert_turn(conn, source_ref, turn, compact_source(state))?;
        mark_dirty(&mut dirty, &turn.session_id, &turn.ended_at);
    }
    for edit in &facts.edits {
        insert_edit(conn, source_ref, edit, compact_source(state))?;
        mark_dirty(&mut dirty, &edit.session_id, &edit.occurred_at);
    }
    for interval in intervals {
        if interval.method == ActivityMethod::Measured {
            insert_interval(conn, state, interval)?;
        } else {
            // Every span this one overlaps is superseded by it, and their hours
            // have to be rebuilt as well as its own.
            for (start, end) in replace_overlapping_clustered(conn, state, interval)? {
                mark_dirty_span(&mut dirty, &interval.session_id, &start, &end);
            }
        }
        // An interval can span hours, and each hour it touches owns a slice of
        // it. Marking only the start bucket would leave later hours stale.
        mark_dirty_span(
            &mut dirty,
            &interval.session_id,
            &interval.started_at,
            &interval.ended_at,
        );
    }
    for limit in &facts.limits {
        insert_limit(conn, limit)?;
    }

    save_source_state_on(conn, state)?;
    Ok(dirty)
}

/// Drop every fact this source produced, reporting the buckets they occupied.
///
/// Needed because `INSERT OR IGNORE` cannot correct a row that already exists.
/// A parser fix re-reads the source to repair facts it previously got wrong, and
/// without this the re-read would collide with exactly the bad rows it was meant
/// to replace, leaving the defect in place and the recovery path decorative.
///
/// Activity intervals go too. It is tempting to keep them on the grounds that
/// re-clustering the same events reproduces the same spans — but that assumes
/// the re-read yields the same events, which is false in precisely the cases
/// this function exists for. A parser fix that changes how timestamps are read,
/// or a log replaced by a different session, produces spans at *different*
/// times; the old rows would not collide, would not be dirtied, and would sit in
/// the store inflating active time forever.
pub(crate) fn purge_source_facts(
    conn: &Connection,
    source_key: &str,
) -> rusqlite::Result<DirtyBuckets> {
    let mut dirty = DirtyBuckets::new();

    for (table, start_column, end_column) in [
        ("telemetry_turns", "ended_at", "ended_at"),
        ("telemetry_edits", "occurred_at", "occurred_at"),
        ("telemetry_activity", "started_at", "ended_at"),
    ] {
        let mut stmt = conn.prepare(&format!(
            "SELECT session_id, {start_column}, {end_column} FROM {table} WHERE source_key = ?1"
        ))?;
        let rows = stmt.query_map(params![source_key], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (session_id, start, end) = row?;
            // A span can cover several hours, and every one of them is about to
            // hold stale numbers.
            mark_dirty_span(&mut dirty, &session_id, &start, &end);
        }
        conn.execute(
            match table {
                "telemetry_turns" => {
                    "DELETE FROM telemetry_turn_facts
                    WHERE source_ref = (SELECT string_id FROM telemetry_strings
                                        WHERE kind = 'source_key' AND value = ?1)"
                }
                "telemetry_edits" => {
                    "DELETE FROM telemetry_edit_facts
                    WHERE source_ref = (SELECT string_id FROM telemetry_strings
                                        WHERE kind = 'source_key' AND value = ?1)"
                }
                _ => "DELETE FROM telemetry_activity WHERE source_key = ?1",
            },
            params![source_key],
        )?;
    }

    Ok(dirty)
}

fn insert_turn(
    conn: &rusqlite::Connection,
    source_ref: i64,
    turn: &TurnFact,
    compact: bool,
) -> rusqlite::Result<()> {
    let session_ref = ensure_string_id(conn, STRING_SESSION, &turn.session_id)?;
    let provider_ref = ensure_string_id(conn, STRING_PROVIDER, &turn.provider)?;
    let model_ref = turn
        .model
        .as_deref()
        .map(|value| ensure_string_id(conn, STRING_MODEL, value))
        .transpose()?;
    let effort_ref = turn
        .effort
        .as_deref()
        .map(|value| ensure_string_id(conn, STRING_EFFORT, value))
        .transpose()?;
    let bucket = compact
        .then(|| five_minute_bucket(&turn.ended_at))
        .flatten();
    let event_key = bucket.as_deref().map_or_else(
        || turn.event_key.clone(),
        |bucket| {
            let turn_key = turn.turn_id.as_deref().unwrap_or(&turn.event_key);
            compact_fact_key(
                "turn",
                &[
                    bucket,
                    turn_key,
                    turn.model.as_deref().unwrap_or(""),
                    turn.effort.as_deref().unwrap_or(""),
                ],
            )
        },
    );
    let ended_at = bucket.as_deref().unwrap_or(&turn.ended_at);
    if !compact || bucket.is_none() {
        conn.execute(
            "INSERT OR IGNORE INTO telemetry_turn_facts (
                 event_key, session_ref, provider_ref, turn_id, model_ref, effort_ref,
                 started_at, ended_at,
                 input_tokens, cached_input_tokens, cache_write_tokens, output_tokens,
                 reasoning_tokens, context_window, cost_usd, source_ref
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                event_key,
                session_ref,
                provider_ref,
                turn.turn_id,
                model_ref,
                effort_ref,
                turn.started_at,
                ended_at,
                turn.input_tokens,
                turn.cached_input_tokens,
                turn.cache_write_tokens,
                turn.output_tokens,
                turn.reasoning_tokens,
                turn.context_window,
                turn.cost_usd,
                source_ref,
            ],
        )?;
    } else if let (Some(bucket), Some(turn_id)) = (bucket.as_deref(), turn.turn_id.as_deref()) {
        let updated = conn.execute(
            "UPDATE telemetry_turn_facts
             SET started_at = COALESCE(started_at, ?1),
                 input_tokens = CASE WHEN input_tokens IS NULL THEN ?2
                                     WHEN ?2 IS NULL THEN input_tokens
                                     ELSE input_tokens + ?2 END,
                 cached_input_tokens = CASE WHEN cached_input_tokens IS NULL THEN ?3
                                            WHEN ?3 IS NULL THEN cached_input_tokens
                                            ELSE cached_input_tokens + ?3 END,
                 cache_write_tokens = CASE WHEN cache_write_tokens IS NULL THEN ?4
                                            WHEN ?4 IS NULL THEN cache_write_tokens
                                            ELSE cache_write_tokens + ?4 END,
                 output_tokens = CASE WHEN output_tokens IS NULL THEN ?5
                                      WHEN ?5 IS NULL THEN output_tokens
                                      ELSE output_tokens + ?5 END,
                 reasoning_tokens = CASE WHEN reasoning_tokens IS NULL THEN ?6
                                         WHEN ?6 IS NULL THEN reasoning_tokens
                                         ELSE reasoning_tokens + ?6 END,
                 context_window = COALESCE(?7, context_window),
                 cost_usd = CASE WHEN cost_usd IS NULL THEN ?8
                                 WHEN ?8 IS NULL THEN cost_usd
                                 ELSE cost_usd + ?8 END
             WHERE source_ref = ?9 AND ended_at = ?10 AND turn_id = ?11
               AND model_ref IS ?12 AND effort_ref IS ?13",
            params![
                turn.started_at,
                turn.input_tokens,
                turn.cached_input_tokens,
                turn.cache_write_tokens,
                turn.output_tokens,
                turn.reasoning_tokens,
                turn.context_window,
                turn.cost_usd,
                source_ref,
                bucket,
                turn_id,
                model_ref,
                effort_ref,
            ],
        )?;
        if updated > 0 {
            return Ok(());
        }
        conn.execute(
            "INSERT INTO telemetry_turn_facts (
                 event_key, session_ref, provider_ref, turn_id, model_ref, effort_ref,
                 started_at, ended_at,
                 input_tokens, cached_input_tokens, cache_write_tokens, output_tokens,
                 reasoning_tokens, context_window, cost_usd, source_ref
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(source_ref, event_key) DO UPDATE SET
                 started_at = COALESCE(telemetry_turn_facts.started_at, excluded.started_at),
                 input_tokens = CASE WHEN telemetry_turn_facts.input_tokens IS NULL THEN excluded.input_tokens
                                     WHEN excluded.input_tokens IS NULL THEN telemetry_turn_facts.input_tokens
                                     ELSE telemetry_turn_facts.input_tokens + excluded.input_tokens END,
                 cached_input_tokens = CASE WHEN telemetry_turn_facts.cached_input_tokens IS NULL THEN excluded.cached_input_tokens
                                            WHEN excluded.cached_input_tokens IS NULL THEN telemetry_turn_facts.cached_input_tokens
                                            ELSE telemetry_turn_facts.cached_input_tokens + excluded.cached_input_tokens END,
                 cache_write_tokens = CASE WHEN telemetry_turn_facts.cache_write_tokens IS NULL THEN excluded.cache_write_tokens
                                            WHEN excluded.cache_write_tokens IS NULL THEN telemetry_turn_facts.cache_write_tokens
                                            ELSE telemetry_turn_facts.cache_write_tokens + excluded.cache_write_tokens END,
                 output_tokens = CASE WHEN telemetry_turn_facts.output_tokens IS NULL THEN excluded.output_tokens
                                      WHEN excluded.output_tokens IS NULL THEN telemetry_turn_facts.output_tokens
                                      ELSE telemetry_turn_facts.output_tokens + excluded.output_tokens END,
                 reasoning_tokens = CASE WHEN telemetry_turn_facts.reasoning_tokens IS NULL THEN excluded.reasoning_tokens
                                         WHEN excluded.reasoning_tokens IS NULL THEN telemetry_turn_facts.reasoning_tokens
                                         ELSE telemetry_turn_facts.reasoning_tokens + excluded.reasoning_tokens END,
                 context_window = COALESCE(excluded.context_window, telemetry_turn_facts.context_window),
                 cost_usd = CASE WHEN telemetry_turn_facts.cost_usd IS NULL THEN excluded.cost_usd
                                 WHEN excluded.cost_usd IS NULL THEN telemetry_turn_facts.cost_usd
                                 ELSE telemetry_turn_facts.cost_usd + excluded.cost_usd END",
            params![
                event_key,
                session_ref,
                provider_ref,
                turn.turn_id,
                model_ref,
                effort_ref,
                turn.started_at,
                ended_at,
                turn.input_tokens,
                turn.cached_input_tokens,
                turn.cache_write_tokens,
                turn.output_tokens,
                turn.reasoning_tokens,
                turn.context_window,
                turn.cost_usd,
                source_ref,
            ],
        )?;
    } else {
        conn.execute(
            "INSERT INTO telemetry_turn_facts (
                 event_key, session_ref, provider_ref, turn_id, model_ref, effort_ref,
                 started_at, ended_at,
                 input_tokens, cached_input_tokens, cache_write_tokens, output_tokens,
                 reasoning_tokens, context_window, cost_usd, source_ref
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(source_ref, event_key) DO UPDATE SET
                 input_tokens = CASE WHEN telemetry_turn_facts.input_tokens IS NULL THEN excluded.input_tokens
                                     WHEN excluded.input_tokens IS NULL THEN telemetry_turn_facts.input_tokens
                                     ELSE telemetry_turn_facts.input_tokens + excluded.input_tokens END,
                 cached_input_tokens = CASE WHEN telemetry_turn_facts.cached_input_tokens IS NULL THEN excluded.cached_input_tokens
                                            WHEN excluded.cached_input_tokens IS NULL THEN telemetry_turn_facts.cached_input_tokens
                                            ELSE telemetry_turn_facts.cached_input_tokens + excluded.cached_input_tokens END,
                 cache_write_tokens = CASE WHEN telemetry_turn_facts.cache_write_tokens IS NULL THEN excluded.cache_write_tokens
                                            WHEN excluded.cache_write_tokens IS NULL THEN telemetry_turn_facts.cache_write_tokens
                                            ELSE telemetry_turn_facts.cache_write_tokens + excluded.cache_write_tokens END,
                 output_tokens = CASE WHEN telemetry_turn_facts.output_tokens IS NULL THEN excluded.output_tokens
                                      WHEN excluded.output_tokens IS NULL THEN telemetry_turn_facts.output_tokens
                                      ELSE telemetry_turn_facts.output_tokens + excluded.output_tokens END,
                 reasoning_tokens = CASE WHEN telemetry_turn_facts.reasoning_tokens IS NULL THEN excluded.reasoning_tokens
                                         WHEN excluded.reasoning_tokens IS NULL THEN telemetry_turn_facts.reasoning_tokens
                                         ELSE telemetry_turn_facts.reasoning_tokens + excluded.reasoning_tokens END,
                 context_window = COALESCE(excluded.context_window, telemetry_turn_facts.context_window),
                 cost_usd = CASE WHEN telemetry_turn_facts.cost_usd IS NULL THEN excluded.cost_usd
                                 WHEN excluded.cost_usd IS NULL THEN telemetry_turn_facts.cost_usd
                                 ELSE telemetry_turn_facts.cost_usd + excluded.cost_usd END",
            params![
                event_key,
                session_ref,
                provider_ref,
                turn.turn_id,
                model_ref,
                effort_ref,
                turn.started_at,
                ended_at,
                turn.input_tokens,
                turn.cached_input_tokens,
                turn.cache_write_tokens,
                turn.output_tokens,
                turn.reasoning_tokens,
                turn.context_window,
                turn.cost_usd,
                source_ref,
            ],
        )?;
    }
    Ok(())
}

fn insert_edit(
    conn: &rusqlite::Connection,
    source_ref: i64,
    edit: &EditFact,
    compact: bool,
) -> rusqlite::Result<()> {
    let session_ref = ensure_string_id(conn, STRING_SESSION, &edit.session_id)?;
    let provider_ref = ensure_string_id(conn, STRING_PROVIDER, &edit.provider)?;
    let bucket = compact
        .then(|| five_minute_bucket(&edit.occurred_at))
        .flatten();
    let event_key = bucket.as_deref().map_or_else(
        || edit.event_key.clone(),
        |bucket| {
            let turn_key = edit.turn_id.as_deref().unwrap_or(&edit.event_key);
            compact_fact_key("edit", &[bucket, turn_key, &edit.path, edit.op.as_str()])
        },
    );
    let occurred_at = bucket.as_deref().unwrap_or(&edit.occurred_at);
    if !compact || bucket.is_none() {
        conn.execute(
            "INSERT OR IGNORE INTO telemetry_edit_facts (
                 event_key, session_ref, provider_ref, turn_id, occurred_at, workspace, path, op,
                 lines_added, lines_removed, source_ref
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                event_key,
                session_ref,
                provider_ref,
                edit.turn_id,
                occurred_at,
                edit.workspace,
                edit.path,
                edit.op.as_str(),
                edit.lines_added,
                edit.lines_removed,
                source_ref,
            ],
        )?;
    } else if let (Some(bucket), Some(turn_id)) = (bucket.as_deref(), edit.turn_id.as_deref()) {
        let updated = conn.execute(
            "UPDATE telemetry_edit_facts
             SET lines_added = CASE WHEN lines_added IS NULL THEN ?1
                                    WHEN ?1 IS NULL THEN lines_added
                                    ELSE lines_added + ?1 END,
                 lines_removed = CASE WHEN lines_removed IS NULL THEN ?2
                                      WHEN ?2 IS NULL THEN lines_removed
                                      ELSE lines_removed + ?2 END
             WHERE source_ref = ?3 AND occurred_at = ?4 AND turn_id = ?5
               AND workspace IS ?6 AND path = ?7 AND op = ?8",
            params![
                edit.lines_added,
                edit.lines_removed,
                source_ref,
                bucket,
                turn_id,
                edit.workspace,
                edit.path,
                edit.op.as_str(),
            ],
        )?;
        if updated > 0 {
            return Ok(());
        }
        conn.execute(
            "INSERT INTO telemetry_edit_facts (
                 event_key, session_ref, provider_ref, turn_id, occurred_at, workspace, path, op,
                 lines_added, lines_removed, source_ref
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(source_ref, event_key) DO UPDATE SET
                 lines_added = CASE WHEN telemetry_edit_facts.lines_added IS NULL THEN excluded.lines_added
                                    WHEN excluded.lines_added IS NULL THEN telemetry_edit_facts.lines_added
                                    ELSE telemetry_edit_facts.lines_added + excluded.lines_added END,
                 lines_removed = CASE WHEN telemetry_edit_facts.lines_removed IS NULL THEN excluded.lines_removed
                                      WHEN excluded.lines_removed IS NULL THEN telemetry_edit_facts.lines_removed
                                      ELSE telemetry_edit_facts.lines_removed + excluded.lines_removed END",
            params![
                event_key,
                session_ref,
                provider_ref,
                edit.turn_id,
                occurred_at,
                edit.workspace,
                edit.path,
                edit.op.as_str(),
                edit.lines_added,
                edit.lines_removed,
                source_ref,
            ],
        )?;
    } else {
        conn.execute(
            "INSERT INTO telemetry_edit_facts (
                 event_key, session_ref, provider_ref, turn_id, occurred_at, workspace, path, op,
                 lines_added, lines_removed, source_ref
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(source_ref, event_key) DO UPDATE SET
                 lines_added = CASE WHEN telemetry_edit_facts.lines_added IS NULL THEN excluded.lines_added
                                    WHEN excluded.lines_added IS NULL THEN telemetry_edit_facts.lines_added
                                    ELSE telemetry_edit_facts.lines_added + excluded.lines_added END,
                 lines_removed = CASE WHEN telemetry_edit_facts.lines_removed IS NULL THEN excluded.lines_removed
                                      WHEN excluded.lines_removed IS NULL THEN telemetry_edit_facts.lines_removed
                                      ELSE telemetry_edit_facts.lines_removed + excluded.lines_removed END",
            params![
                event_key,
                session_ref,
                provider_ref,
                edit.turn_id,
                occurred_at,
                edit.workspace,
                edit.path,
                edit.op.as_str(),
                edit.lines_added,
                edit.lines_removed,
                source_ref,
            ],
        )?;
    }
    Ok(())
}

/// Providers with byte-offset cursors can replay only bytes after the stored
/// offset. Their fact write and cursor advance share one transaction, so a
/// compact upsert is safe without retaining every provider event key.
fn compact_source(state: &SourceState) -> bool {
    state.cursor.kind == CursorKind::ByteOffset
        && matches!(state.provider.as_str(), "codex" | "claude" | "pi")
}

/// Floor an RFC 3339 instant to the finest bucket rendered by Analytics.
fn five_minute_bucket(timestamp: &str) -> Option<String> {
    let instant = crate::telemetry::activity::parse_timestamp(timestamp)?;
    instant
        .with_minute((instant.minute() / 5) * 5)
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn insert_interval(
    conn: &rusqlite::Connection,
    state: &SourceState,
    interval: &IntervalFact,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO telemetry_activity (
             session_id, provider, started_at, ended_at, last_event_at, event_count, method,
             source_key
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            interval.session_id,
            interval.provider,
            interval.started_at,
            interval.ended_at,
            interval.last_event_at,
            interval.event_count,
            interval.method.as_str(),
            state.source_key,
        ],
    )?;
    Ok(())
}

/// Store a clustered interval, replacing every clustered span it overlaps.
///
/// Returns the spans that were removed, so their hours can be rebuilt.
///
/// Replacement rather than insertion is what keeps clustered activity coherent.
/// A cluster is not a fixed object: later events widen it, and an event that
/// arrives out of order can move its *start* earlier. Inserting would then leave
/// the superseded row in place — uniqueness is on `started_at`, so a new earlier
/// start does not collide — and the rollup would clip both, counting the shared
/// minutes twice.
fn replace_overlapping_clustered(
    conn: &rusqlite::Connection,
    state: &SourceState,
    interval: &IntervalFact,
) -> rusqlite::Result<Vec<(String, String)>> {
    // Half-open comparison is deliberate: two spans that merely touch at an
    // instant do not overlap, and merging them would be wrong.
    const OVERLAP: &str = "session_id = ?1 AND provider = ?2 AND method != 'measured'
         AND started_at <= ?3 AND ended_at >= ?4";

    let mut stmt = conn.prepare(&format!(
        "SELECT started_at, ended_at FROM telemetry_activity WHERE {OVERLAP}"
    ))?;
    let superseded = stmt
        .query_map(
            params![
                interval.session_id,
                interval.provider,
                interval.ended_at,
                interval.started_at
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    conn.execute(
        &format!("DELETE FROM telemetry_activity WHERE {OVERLAP}"),
        params![
            interval.session_id,
            interval.provider,
            interval.ended_at,
            interval.started_at
        ],
    )?;

    insert_interval(conn, state, interval)?;
    Ok(superseded)
}

fn insert_limit(conn: &rusqlite::Connection, limit: &LimitObservation) -> rusqlite::Result<()> {
    // The UI exposes this as a current account gauge, not a historical chart.
    // Keep one observation per provider so a frequent provider heartbeat cannot
    // become another unbounded append-only table. Older observations from a
    // parser re-read are ignored by timestamp rather than replacing the gauge
    // with stale data.
    let newest: Option<String> = conn
        .query_row(
            "SELECT observed_at FROM telemetry_limits
             WHERE provider = ?1
             ORDER BY observed_at DESC, id DESC
             LIMIT 1",
            params![limit.provider],
            |row| row.get(0),
        )
        .optional()?;
    if newest
        .as_deref()
        .is_some_and(|observed_at| observed_at >= limit.observed_at.as_str())
    {
        return Ok(());
    }

    conn.execute(
        "DELETE FROM telemetry_limits WHERE provider = ?1",
        params![limit.provider],
    )?;
    conn.execute(
        "INSERT INTO telemetry_limits (
             provider, limit_id, observed_at, used_percent, window_minutes, resets_at, plan_type
         ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            limit.provider,
            limit.limit_id,
            limit.observed_at,
            limit.used_percent,
            limit.window_minutes,
            limit.resets_at,
            limit.plan_type,
        ],
    )?;
    Ok(())
}

/// The most recent stored clustered interval for a session.
///
/// Clustering runs per delta, but a work session does not stop at a delta
/// boundary. Without this anchor, every ingest cycle would start a fresh
/// cluster and a single continuous stretch of work would fragment into
/// disconnected singletons — inflating interval counts and shredding the
/// timeline into slivers.
pub(crate) fn last_clustered_interval(
    conn: &Connection,
    session_id: &str,
    provider: &str,
) -> rusqlite::Result<Option<IntervalFact>> {
    conn.query_row(
        "SELECT session_id, provider, started_at, ended_at, last_event_at, event_count, method
         FROM telemetry_activity
         WHERE session_id = ?1 AND provider = ?2 AND method != 'measured'
         ORDER BY started_at DESC
         LIMIT 1",
        params![session_id, provider],
        |row| {
            Ok(IntervalFact {
                session_id: row.get(0)?,
                provider: row.get(1)?,
                started_at: row.get(2)?,
                ended_at: row.get(3)?,
                last_event_at: row.get(4)?,
                event_count: row.get(5)?,
                method: ActivityMethod::parse(&row.get::<_, String>(6)?)
                    .unwrap_or(ActivityMethod::Clustered),
            })
        },
    )
    .optional()
}

pub(crate) fn mark_dirty(dirty: &mut DirtyBuckets, session_id: &str, timestamp: &str) {
    if let Some(bucket) = hour_bucket(timestamp) {
        dirty.insert((bucket, session_id.to_string()));
    }
}

/// Guards against a malformed interval producing an unbounded bucket walk.
const MAX_SPAN_BUCKETS: usize = 24 * 40;

pub(crate) fn mark_dirty_span(
    dirty: &mut DirtyBuckets,
    session_id: &str,
    started_at: &str,
    ended_at: &str,
) {
    let (Some(start), Some(end)) = (hour_bucket(started_at), hour_bucket(ended_at)) else {
        return;
    };
    let mut bucket = start;
    let mut guard = 0;
    loop {
        dirty.insert((bucket.clone(), session_id.to_string()));
        if bucket >= end || guard >= MAX_SPAN_BUCKETS {
            break;
        }
        bucket = crate::telemetry::rollup::next_hour(&bucket);
        guard += 1;
    }
}

/// Truncate an RFC 3339 timestamp to its hour bucket.
pub fn hour_bucket(timestamp: &str) -> Option<String> {
    let parsed = crate::telemetry::activity::parse_timestamp(timestamp)?;
    Some(parsed.format("%Y-%m-%dT%H:00:00.000Z").to_string())
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::models::EditOp;
    use crate::telemetry::schema::run_telemetry_migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        conn
    }

    fn state() -> SourceState {
        SourceState {
            source_key: source_key("codex", "agent-1", "rollout.jsonl"),
            source_path: "rollout.jsonl".into(),
            session_id: "agent-1".into(),
            provider_session_id: None,
            provider: "codex".into(),
            source_kind: SourceKind::Jsonl,
            cursor: Cursor::new(CursorKind::ByteOffset, 128),
            last_size: 128,
            last_modified: None,
            parser_version: 1,
            fingerprint: None,
            carry: SourceCarry::default(),
        }
    }

    fn turn(ended_at: &str, turn_id: &str) -> TurnFact {
        TurnFact {
            event_key: format!("turn:{turn_id}:{ended_at}"),
            session_id: "agent-1".into(),
            provider: "codex".into(),
            turn_id: Some(turn_id.into()),
            model: Some("gpt-5.6-terra".into()),
            effort: None,
            started_at: None,
            ended_at: ended_at.into(),
            input_tokens: Some(100),
            cached_input_tokens: Some(50),
            cache_write_tokens: Some(0),
            output_tokens: Some(10),
            reasoning_tokens: Some(5),
            context_window: None,
            cost_usd: None,
        }
    }

    #[test]
    fn source_state_round_trips() {
        let conn = db();
        save_source_state(&conn, &state()).unwrap();
        let loaded = load_source_state(&conn, &source_key("codex", "agent-1", "rollout.jsonl"))
            .unwrap()
            .unwrap();
        assert_eq!(loaded.cursor, Cursor::new(CursorKind::ByteOffset, 128));
        assert_eq!(loaded.provider, "codex");
        assert_eq!(loaded.source_kind, SourceKind::Jsonl);
    }

    #[test]
    fn sqlite_source_state_keeps_its_cursor_kind() {
        let conn = db();
        let mut sqlite_state = state();
        sqlite_state.source_key = source_key("opencode", "agent-1", "opencode.db");
        sqlite_state.source_path = "opencode.db".into();
        sqlite_state.provider = "opencode".into();
        sqlite_state.source_kind = SourceKind::Sqlite;
        sqlite_state.cursor = Cursor::new(CursorKind::EpochMs, 1786644345306);
        save_source_state(&conn, &sqlite_state).unwrap();

        let loaded = load_source_state(&conn, &sqlite_state.source_key)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.cursor.kind, CursorKind::EpochMs);
        assert_eq!(loaded.cursor.value, 1786644345306);
    }

    #[test]
    fn unknown_source_loads_as_none() {
        assert!(load_source_state(&db(), "missing").unwrap().is_none());
    }

    #[test]
    fn agents_sharing_one_database_do_not_share_a_cursor() {
        // The regression this guards: opencode keeps a single database for the
        // whole machine. Keyed by path alone, whichever agent ingested first
        // would leave its high-water mark as the cursor, and the next agent
        // would resume from a position in someone else's history — skipping its
        // own past permanently, since the cursor only moves forward.
        let conn = db();
        for (session, cursor) in [("agent-a", 9_000_i64), ("agent-b", 1_000)] {
            let mut shared = state();
            shared.source_key = source_key("opencode", session, "opencode.db");
            shared.source_path = "opencode.db".into();
            shared.session_id = session.into();
            shared.provider = "opencode".into();
            shared.source_kind = SourceKind::Sqlite;
            shared.cursor = Cursor::new(CursorKind::EpochMs, cursor);
            save_source_state(&conn, &shared).unwrap();
        }

        let b = load_source_state(&conn, &source_key("opencode", "agent-b", "opencode.db"))
            .unwrap()
            .unwrap();
        assert_eq!(b.cursor.value, 1_000);
        assert_eq!(b.session_id, "agent-b");
    }

    #[test]
    fn carry_context_survives_a_round_trip() {
        // Codex states a turn's model once and lets later records inherit it, so
        // this is what keeps attribution identical across a delta boundary.
        let conn = db();
        let mut carried = state();
        carried.carry = SourceCarry {
            turn_id: Some("t-7".into()),
            model: Some("gpt-5.6-terra".into()),
            effort: Some("high".into()),
            cwd: Some("D:/repo".into()),
        };
        save_source_state(&conn, &carried).unwrap();

        let loaded = load_source_state(&conn, &carried.source_key)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.carry, carried.carry);
    }

    #[test]
    fn writing_facts_advances_the_cursor_atomically() {
        let conn = db();
        let facts = ParsedFacts {
            turns: vec![turn("2026-08-13T18:42:49.628Z", "t1")],
            ..Default::default()
        };
        write_facts(&conn, &facts, &[], &state()).unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let loaded = load_source_state(&conn, &source_key("codex", "agent-1", "rollout.jsonl"))
            .unwrap()
            .unwrap();
        assert_eq!(loaded.cursor.value, 128);
    }

    #[test]
    fn re_ingesting_the_same_delta_does_not_double_count() {
        let conn = db();
        let facts = ParsedFacts {
            turns: vec![turn("2026-08-13T18:42:49.628Z", "t1")],
            edits: vec![EditFact {
                event_key: "edit:1".into(),
                session_id: "agent-1".into(),
                provider: "codex".into(),
                turn_id: Some("t1".into()),
                occurred_at: "2026-08-13T18:42:49.628Z".into(),
                workspace: None,
                path: "a.md".into(),
                op: EditOp::Add,
                lines_added: Some(3),
                lines_removed: Some(0),
            }],
            ..Default::default()
        };
        write_facts(&conn, &facts, &[], &state()).unwrap();
        write_facts(&conn, &facts, &[], &state()).unwrap();

        let turns: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turns", [], |row| row.get(0))
            .unwrap();
        let edits: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_edits", [], |row| row.get(0))
            .unwrap();
        assert_eq!(turns, 1);
        assert_eq!(edits, 1);
    }

    #[test]
    fn byte_offset_turns_coalesce_inside_the_five_minute_interface_cell() {
        let conn = db();
        let first = turn("2026-08-13T18:42:00.000Z", "t1");
        let second = turn("2026-08-13T18:44:00.000Z", "t1");
        write_facts(
            &conn,
            &ParsedFacts {
                turns: vec![first],
                ..Default::default()
            },
            &[],
            &state(),
        )
        .unwrap();

        let mut next_state = state();
        next_state.cursor.value = 256;
        write_facts(
            &conn,
            &ParsedFacts {
                turns: vec![second],
                ..Default::default()
            },
            &[],
            &next_state,
        )
        .unwrap();

        let (rows, input, output): (i64, i64, i64) = conn
            .query_row(
                "SELECT count(*), SUM(input_tokens), SUM(output_tokens)
                 FROM telemetry_turns",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(input, 200);
        assert_eq!(output, 20);
    }

    #[test]
    fn limit_observations_are_kept_as_one_current_gauge_per_provider() {
        let conn = db();
        let write = |observed_at: &str, used_percent: f64| {
            write_facts(
                &conn,
                &ParsedFacts {
                    limits: vec![LimitObservation {
                        provider: "codex".into(),
                        limit_id: Some("primary".into()),
                        observed_at: observed_at.into(),
                        used_percent: Some(used_percent),
                        window_minutes: Some(300),
                        resets_at: None,
                        plan_type: Some("pro".into()),
                    }],
                    ..Default::default()
                },
                &[],
                &state(),
            )
            .unwrap();
        };

        write("2026-08-30T10:00:00.000Z", 21.0);
        write("2026-08-30T10:01:00.000Z", 43.0);
        write("2026-08-30T09:59:00.000Z", 7.0);

        let (count, used): (i64, f64) = conn
            .query_row(
                "SELECT count(*), used_percent FROM telemetry_limits
                 WHERE provider = 'codex'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(used, 43.0);
    }

    #[test]
    fn dirty_buckets_are_reported_per_hour() {
        let conn = db();
        let facts = ParsedFacts {
            turns: vec![
                turn("2026-08-13T18:42:49.628Z", "t1"),
                turn("2026-08-13T19:05:00.000Z", "t2"),
            ],
            ..Default::default()
        };
        let dirty = write_facts(&conn, &facts, &[], &state()).unwrap();
        assert_eq!(dirty.len(), 2);
        assert!(dirty.contains(&(
            "2026-08-13T18:00:00.000Z".to_string(),
            "agent-1".to_string()
        )));
    }

    #[test]
    fn hour_bucket_truncates() {
        assert_eq!(
            hour_bucket("2026-08-13T18:42:49.628Z").as_deref(),
            Some("2026-08-13T18:00:00.000Z")
        );
    }

    #[test]
    fn clustered_interval_widens_as_more_events_arrive() {
        let conn = db();
        let short = IntervalFact {
            session_id: "agent-1".into(),
            provider: "codex".into(),
            started_at: "2026-08-13T18:00:00.000Z".into(),
            ended_at: "2026-08-13T18:05:00.000Z".into(),
            last_event_at: "2026-08-13T18:05:00.000Z".into(),
            event_count: 2,
            method: ActivityMethod::Clustered,
        };
        write_facts(
            &conn,
            &ParsedFacts::default(),
            std::slice::from_ref(&short),
            &state(),
        )
        .unwrap();

        let longer = IntervalFact {
            ended_at: "2026-08-13T18:09:00.000Z".into(),
            last_event_at: "2026-08-13T18:09:00.000Z".into(),
            event_count: 4,
            ..short
        };
        write_facts(
            &conn,
            &ParsedFacts::default(),
            std::slice::from_ref(&longer),
            &state(),
        )
        .unwrap();

        let (rows, ended_at, events): (i64, String, i64) = conn
            .query_row(
                "SELECT count(*), MAX(ended_at), MAX(event_count) FROM telemetry_activity",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(rows, 1, "the wider span replaces the earlier one");
        assert_eq!(ended_at, "2026-08-13T18:09:00.000Z");
        assert_eq!(events, 4);
    }

    #[test]
    fn a_late_event_moving_a_cluster_start_does_not_leave_an_overlap() {
        // Uniqueness is on `started_at`, so a cluster that gains an earlier
        // start does not collide with its own previous row. Inserting would
        // leave both, and the rollup clips both — counting the overlapping
        // minutes twice.
        let conn = db();
        let original = IntervalFact {
            session_id: "agent-1".into(),
            provider: "codex".into(),
            started_at: "2026-08-13T18:00:00.000Z".into(),
            ended_at: "2026-08-13T18:05:00.000Z".into(),
            last_event_at: "2026-08-13T18:05:00.000Z".into(),
            event_count: 2,
            method: ActivityMethod::Clustered,
        };
        write_facts(
            &conn,
            &ParsedFacts::default(),
            std::slice::from_ref(&original),
            &state(),
        )
        .unwrap();

        // A record arrives out of order, before the stored start.
        let widened = IntervalFact {
            started_at: "2026-08-13T17:55:00.000Z".into(),
            event_count: 3,
            ..original
        };
        let dirty = write_facts(
            &conn,
            &ParsedFacts::default(),
            std::slice::from_ref(&widened),
            &state(),
        )
        .unwrap();

        let (rows, started_at): (i64, String) = conn
            .query_row(
                "SELECT count(*), MIN(started_at) FROM telemetry_activity",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(started_at, "2026-08-13T17:55:00.000Z");
        // The superseded span's hour must be rebuilt too, not just the new one.
        assert!(dirty.contains(&(
            "2026-08-13T18:00:00.000Z".to_string(),
            "agent-1".to_string()
        )));
        assert!(dirty.contains(&(
            "2026-08-13T17:00:00.000Z".to_string(),
            "agent-1".to_string()
        )));
    }

    #[test]
    fn measured_intervals_are_never_replaced_by_clustered_ones() {
        // Measured spans are facts the provider reported; a clustered estimate
        // overlapping one must never delete it.
        let conn = db();
        let measured = IntervalFact {
            session_id: "agent-1".into(),
            provider: "opencode".into(),
            started_at: "2026-08-13T18:00:00.000Z".into(),
            ended_at: "2026-08-13T18:05:00.000Z".into(),
            last_event_at: "2026-08-13T18:05:00.000Z".into(),
            event_count: 1,
            method: ActivityMethod::Measured,
        };
        let mut opencode_state = state();
        opencode_state.provider = "opencode".into();
        write_facts(
            &conn,
            &ParsedFacts::default(),
            std::slice::from_ref(&measured),
            &opencode_state,
        )
        .unwrap();

        let overlapping = IntervalFact {
            started_at: "2026-08-13T17:50:00.000Z".into(),
            ended_at: "2026-08-13T18:30:00.000Z".into(),
            last_event_at: "2026-08-13T18:30:00.000Z".into(),
            event_count: 5,
            method: ActivityMethod::Clustered,
            ..measured
        };
        write_facts(
            &conn,
            &ParsedFacts::default(),
            std::slice::from_ref(&overlapping),
            &opencode_state,
        )
        .unwrap();

        let measured_rows: i64 = conn
            .query_row(
                "SELECT count(*) FROM telemetry_activity WHERE method = 'measured'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(measured_rows, 1);
    }

    #[test]
    fn purging_a_source_removes_its_activity_and_dirties_those_hours() {
        // Keeping intervals on a re-read assumes re-clustering reproduces them,
        // which is false exactly when the re-read exists: a parser fix or a
        // replaced log yields spans at different times, so the old rows would
        // never collide, never be dirtied, and inflate active time forever.
        let conn = db();
        let interval = IntervalFact {
            session_id: "agent-1".into(),
            provider: "codex".into(),
            started_at: "2026-08-13T18:40:00.000Z".into(),
            ended_at: "2026-08-13T19:10:00.000Z".into(),
            last_event_at: "2026-08-13T19:10:00.000Z".into(),
            event_count: 4,
            method: ActivityMethod::Clustered,
        };
        write_facts(
            &conn,
            &ParsedFacts::default(),
            std::slice::from_ref(&interval),
            &state(),
        )
        .unwrap();

        let dirty = purge_source_facts(&conn, &state().source_key).unwrap();

        let rows: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_activity", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 0);
        // The span crossed an hour boundary, so both hours are stale.
        assert!(dirty.contains(&(
            "2026-08-13T18:00:00.000Z".to_string(),
            "agent-1".to_string()
        )));
        assert!(dirty.contains(&(
            "2026-08-13T19:00:00.000Z".to_string(),
            "agent-1".to_string()
        )));
    }
}

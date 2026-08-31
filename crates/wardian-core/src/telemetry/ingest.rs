//! Ingest orchestration.
//!
//! Advancing a source is four steps that must happen in this order: read the
//! delta, derive intervals, write facts and the cursor in one transaction, then
//! recompute the buckets that changed.
//!
//! The transaction boundary is the important part. A crash must never leave the
//! cursor past facts that were not written, because those facts would then be
//! skipped forever — the cursor only ever moves on the far side of a successful
//! commit.

use crate::telemetry::activity::cluster_events;
use crate::telemetry::identity::canonical_path;
use crate::telemetry::models::{ActivityMethod, Cursor, IntervalFact, ParsedFacts};
use crate::telemetry::rollup::recompute_buckets;
use crate::telemetry::schema::acquire_telemetry_lock;
use crate::telemetry::sources::{source_for, SourceContext, SourceError};
use crate::telemetry::store::{
    last_clustered_interval, load_source_state, purge_source_facts, source_key, write_facts,
    SourceState,
};
use rusqlite::Connection;

/// What one source advance accomplished.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IngestOutcome {
    pub turns: usize,
    pub edits: usize,
    pub intervals: usize,
    pub limits: usize,
    pub buckets_recomputed: usize,
    pub cursor_before: i64,
    pub cursor_after: i64,
}

impl IngestOutcome {
    pub fn advanced(&self) -> bool {
        self.cursor_after != self.cursor_before
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("no telemetry source for provider {0}")]
    UnsupportedProvider(String),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("telemetry store error: {0}")]
    Store(String),
}

impl From<rusqlite::Error> for IngestError {
    fn from(err: rusqlite::Error) -> Self {
        IngestError::Store(err.to_string())
    }
}

/// Advance one source and fold its facts into the store.
///
/// Takes `&Connection`, not `&mut Connection`, because the application reaches
/// its database through [`crate::db::get_db_conn`], which lends a shared
/// reference from behind a global mutex. Requiring `&mut` would make this
/// uncallable from the app that exists to call it.
pub fn ingest_source(conn: &Connection, ctx: &SourceContext) -> Result<IngestOutcome, IngestError> {
    let source = source_for(&ctx.provider)
        .ok_or_else(|| IngestError::UnsupportedProvider(ctx.provider.clone()))?;
    // Migration and maintenance release their SQLite transaction between
    // batches. The adjacent lease keeps a current app writer from entering
    // that gap; legacy binaries are fenced by the migration's exclusive
    // locking mode instead.
    let _telemetry_lock = acquire_telemetry_lock(conn)?;

    // A projected habitat may reach the same provider log through a junction
    // or symlink. Persist the physical path so those spellings share one
    // cursor and one `(source, event)` uniqueness domain.
    let source_path = canonical_path(&ctx.path).to_string_lossy().to_string();
    let key = source_key(&ctx.provider, &ctx.session_id, &source_path);
    let existing = load_source_state(conn, &key)?;

    // Two reasons to start over. A parser version bump means previously
    // extracted facts may be wrong; a changed fingerprint means the cursor
    // refers to bytes that are no longer there. Uniqueness constraints absorb
    // the rows that were already correct, so re-reading costs only time.
    let stale = existing.as_ref().is_some_and(|state| {
        state.parser_version != source.parser_version()
            || source.cursor_is_stale(ctx, state.fingerprint.as_deref())
    });
    let resume = existing.as_ref().filter(|_| !stale);

    let cursor = resume.map_or_else(|| Cursor::start(source.cursor_kind()), |state| state.cursor);
    // Carry is parser state for the bytes after the cursor, so resetting the
    // cursor must reset it too or the re-read would inherit context from
    // records it is about to parse again.
    let carry = resume.map(|state| state.carry.clone()).unwrap_or_default();
    let cursor_before = cursor.value;

    let (facts, next_cursor) = source.read_since(ctx, cursor, carry)?;

    let (last_size, last_modified) = file_stats(&ctx.path);
    let state = SourceState {
        source_key: key.clone(),
        source_path: source_path.clone(),
        session_id: ctx.session_id.clone(),
        provider_session_id: ctx.primary_session_id(),
        provider: ctx.provider.clone(),
        source_kind: source.source_kind(),
        cursor: next_cursor,
        last_size,
        last_modified,
        parser_version: source.parser_version(),
        // Keep the last known identity rather than forgetting it. A source whose
        // first line is momentarily unreadable would otherwise have its
        // fingerprint cleared while its cursor stayed put, and the next pass
        // would have nothing left to compare against.
        fingerprint: source.fingerprint(ctx).or_else(|| {
            existing
                .as_ref()
                .and_then(|state| state.fingerprint.clone())
        }),
        carry: facts.carry.clone(),
    };

    // Everything below commits together. Splitting the fact write from the
    // rollup rebuild would let a crash in between strand the cursor ahead of
    // buckets that were never recomputed, and because the cursor never moves
    // back, no later pass would notice they were stale.
    // IMMEDIATE, not the default DEFERRED. This transaction reads (to find the
    // clustering anchor) before it writes, and a deferred transaction takes the
    // write lock only at that first write. Under WAL, upgrading a read to a
    // write after another connection has committed fails with
    // SQLITE_BUSY_SNAPSHOT — an error no amount of waiting resolves, because the
    // snapshot is already stale. Taking the lock up front makes concurrent
    // ingests queue instead of race, which is what a phase-2 scheduler
    // advancing several agents at once will do.
    // `new_unchecked` because the connection is shared, not owned; the caller's
    // mutex is what guarantees no other transaction is open on it.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;

    // A re-read exists to repair facts, and `INSERT OR IGNORE` cannot repair
    // anything, so the rows being replaced have to go first.
    let mut dirty = if stale {
        purge_source_facts(&tx, &key)?
    } else {
        Default::default()
    };

    let intervals = derive_intervals(&tx, ctx, &facts)?;
    // Writing a clustered interval replaces every stored span it overlaps, so
    // widening and out-of-order arrival are the same operation rather than two.
    dirty.extend(write_facts(&tx, &facts, &intervals, &state)?);
    recompute_buckets(&tx, &dirty)?;
    tx.commit()?;

    Ok(IngestOutcome {
        turns: facts.turns.len(),
        edits: facts.edits.len(),
        intervals: intervals.len(),
        limits: facts.limits.len(),
        buckets_recomputed: dirty.len(),
        cursor_before,
        cursor_after: next_cursor.value,
    })
}

/// Turn a source's output into activity intervals.
///
/// A source supplies measured intervals *or* bare event timestamps. Honouring
/// both would double count, so measured intervals win and clustering runs only
/// when the provider could not report durations itself.
///
/// Clustering is anchored on the last stored interval so a work session that
/// straddles a delta boundary stays one interval instead of fragmenting.
fn derive_intervals(
    conn: &Connection,
    ctx: &SourceContext,
    facts: &ParsedFacts,
) -> Result<Vec<IntervalFact>, IngestError> {
    if !facts.intervals.is_empty() {
        return Ok(facts.intervals.clone());
    }
    if facts.event_times.is_empty() {
        return Ok(Vec::new());
    }

    let anchor = last_clustered_interval(conn, &ctx.session_id, &ctx.provider)?;
    let mut event_times = facts.event_times.clone();
    if let Some(anchor) = &anchor {
        // A synthetic event at the stored interval's *last real event* lets the
        // clusterer decide, by its own gap rule, whether the new events continue
        // it. Using `ended_at` would place the synthetic event up to 40s later
        // than anything that actually happened, so an event just past the gap
        // threshold from the real one would fall just inside it from the
        // credited one — clustering the same log differently depending on when
        // ingest happened to run.
        event_times.push(anchor.last_event_at.clone());
    }

    let mut intervals = cluster_events(
        &ctx.session_id,
        &ctx.provider,
        &event_times,
        ActivityMethod::Clustered,
    );

    if let Some(anchor) = anchor {
        merge_anchor(&mut intervals, &anchor);
    }
    Ok(intervals)
}

/// Reconcile the synthetic anchor event with the clusters it produced.
///
/// Two outcomes. If new events joined the anchor's cluster, the work continued
/// across the delta boundary: the anchor's true start and event count are folded
/// back in, widening the stored interval.
///
/// If nothing joined it, the anchor sits alone in a cluster of one — the new
/// events were far enough away to start their own. That lone cluster is not an
/// interval, it is an artefact of asking the question, and it is discarded. Left
/// in, it would be written back over the stored interval as a singleton and
/// stretch it by the singleton credit, quietly inflating clustered active time
/// every time a session resumed after a break.
fn merge_anchor(intervals: &mut Vec<IntervalFact>, anchor: &IntervalFact) {
    let Some(index) = intervals.iter().position(|interval| {
        interval.started_at <= anchor.last_event_at && anchor.last_event_at <= interval.ended_at
    }) else {
        return;
    };

    if intervals[index].event_count <= 1 {
        intervals.remove(index);
        return;
    }

    let interval = &mut intervals[index];
    if anchor.started_at < interval.started_at {
        interval.started_at = anchor.started_at.clone();
    }
    if anchor.ended_at > interval.ended_at {
        interval.ended_at = anchor.ended_at.clone();
        interval.last_event_at = anchor.last_event_at.clone();
    }
    interval.event_count = interval.event_count - 1 + anchor.event_count;
}

fn file_stats(path: &std::path::Path) -> (i64, Option<String>) {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let modified = meta.modified().ok().map(|time| {
                chrono::DateTime::<chrono::Utc>::from(time)
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            });
            (meta.len() as i64, modified)
        }
        Err(_) => (0, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::schema::run_telemetry_migrations;
    use rusqlite::params;
    use std::io::Write;

    const TURN_CONTEXT: &str = r#"{"timestamp":"2026-08-13T18:40:46.104Z","type":"turn_context","payload":{"turn_id":"t-1","cwd":"D:\\repo","model":"gpt-5.6-terra","effort":"high"}}"#;

    /// A token_count record whose stored input equals `input`.
    ///
    /// `cached_input_tokens` is deliberately zero: the codex source subtracts
    /// cache reads from the prompt total, so any other value would make every
    /// test in this module assert a number different from the one it passed in.
    /// These tests are about cursors, deduplication, and parser bumps — the
    /// subtraction itself is covered in `sources::codex` and against a real log.
    fn token_count(timestamp: &str, input: i64) -> String {
        format!(
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":999999}},"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"cache_write_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":2}},"model_context_window":258400}}}}}}"#
        )
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        conn
    }

    fn write_log(dir: &std::path::Path, contents: &str) -> std::path::PathBuf {
        let path = dir.join("rollout.jsonl");
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn codex_ctx(path: &std::path::Path) -> SourceContext {
        SourceContext::new("agent-1", "codex", path)
    }

    #[test]
    fn unsupported_provider_is_refused() {
        // `mock` rather than a real provider: everything Wardian actually runs
        // now resolves to a source, either a native reader or the conversation
        // archive. The mock provider is excluded on purpose, so that the test
        // suite's own agents never land in a habitat's history.
        let conn = db();
        let ctx = SourceContext::new("agent-1", "mock", std::path::Path::new("x.jsonl"));
        let err = ingest_source(&conn, &ctx).unwrap_err();
        assert!(matches!(err, IngestError::UnsupportedProvider(_)));
    }

    #[test]
    fn first_pass_ingests_and_advances() {
        let dir = tempfile::tempdir().unwrap();
        let contents = format!(
            "{TURN_CONTEXT}\n{}\n",
            token_count("2026-08-13T18:42:49.628Z", 100)
        );
        let path = write_log(dir.path(), &contents);
        let conn = db();

        let outcome = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(outcome.turns, 1);
        assert!(outcome.advanced());
        assert!(outcome.buckets_recomputed >= 1);
    }

    #[test]
    fn a_second_pass_with_no_new_bytes_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let contents = format!(
            "{TURN_CONTEXT}\n{}\n",
            token_count("2026-08-13T18:42:49.628Z", 100)
        );
        let path = write_log(dir.path(), &contents);
        let conn = db();

        ingest_source(&conn, &codex_ctx(&path)).unwrap();
        let second = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(second.turns, 0);
        assert!(!second.advanced());

        let turns: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(turns, 1);
    }

    #[test]
    fn appended_lines_are_picked_up_without_re_reading() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!(
                "{TURN_CONTEXT}\n{}\n",
                token_count("2026-08-13T18:42:49.628Z", 100)
            ),
        );
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", token_count("2026-08-13T18:45:00.000Z", 200)).unwrap();

        let outcome = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(outcome.turns, 1);

        let total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(input_tokens),0) FROM telemetry_turns",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(total, 300);
    }

    #[test]
    fn rotation_is_detected_and_re_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!(
                "{TURN_CONTEXT}\n{}\n",
                token_count("2026-08-13T18:42:49.628Z", 100)
            ),
        );
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        // Replaced by a shorter file: the recorded offset now points past EOF.
        let replacement = format!("{}\n", token_count("2026-08-13T19:00:00.000Z", 7));
        std::fs::write(&path, &replacement).unwrap();
        let outcome = ingest_source(&conn, &codex_ctx(&path)).unwrap();

        assert_eq!(outcome.turns, 1);
        // Re-read from the start of the new file, not resumed mid-way through it.
        assert_eq!(outcome.cursor_before, 0);
        assert_eq!(outcome.cursor_after, replacement.len() as i64);
    }

    #[test]
    fn a_replacement_that_is_not_shorter_is_still_detected() {
        // The gap a length check leaves: a file replaced by one the same size or
        // larger resumes at the old offset, silently dropping everything before
        // it. Only the file's identity catches this.
        let dir = tempfile::tempdir().unwrap();
        let original = format!(
            "{TURN_CONTEXT}\n{}\n",
            token_count("2026-08-13T18:42:49.628Z", 100)
        );
        let path = write_log(dir.path(), &original);
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        // A different session, deliberately longer than the original.
        let replacement = format!(
            "{{\"timestamp\":\"2026-08-13T19:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"other-session-with-a-longer-identifier\"}}}}\n{}\n{}\n",
            token_count("2026-08-13T19:00:00.000Z", 7),
            token_count("2026-08-13T19:01:00.000Z", 9),
        );
        assert!(replacement.len() > original.len());
        std::fs::write(&path, &replacement).unwrap();

        let outcome = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(outcome.cursor_before, 0);
        assert_eq!(outcome.turns, 2);
        assert_eq!(outcome.cursor_after, replacement.len() as i64);
    }

    #[test]
    fn ordinary_growth_is_never_mistaken_for_a_replacement() {
        // The other side of the same check: if appending looked like a
        // replacement, every cycle would re-read the log from byte zero.
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!(
                "{TURN_CONTEXT}\n{}\n",
                token_count("2026-08-13T18:00:00.000Z", 10)
            ),
        );
        let conn = db();
        let first = ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", token_count("2026-08-13T18:01:00.000Z", 20)).unwrap();

        let second = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(second.cursor_before, first.cursor_after);
        assert_eq!(second.turns, 1);
    }

    #[test]
    fn a_partial_trailing_line_is_not_ingested_until_complete() {
        let dir = tempfile::tempdir().unwrap();
        let full = token_count("2026-08-13T18:42:49.628Z", 100);
        let partial = &full[..40];
        let path = write_log(dir.path(), &format!("{TURN_CONTEXT}\n{partial}"));
        let conn = db();

        let first = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(first.turns, 0);

        // The writer finishes the line.
        std::fs::write(&path, format!("{TURN_CONTEXT}\n{full}\n")).unwrap();
        let second = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(second.turns, 1);
    }

    #[test]
    fn codex_events_become_clustered_intervals() {
        let dir = tempfile::tempdir().unwrap();
        let contents = format!(
            "{}\n{}\n",
            token_count("2026-08-13T18:00:00.000Z", 100),
            token_count("2026-08-13T18:05:00.000Z", 100)
        );
        let path = write_log(dir.path(), &contents);
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let method: String = conn
            .query_row("SELECT method FROM telemetry_activity", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(method, "clustered");

        let clustered: i64 = conn
            .query_row(
                "SELECT clustered_active_ms FROM telemetry_rollup_hourly",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(clustered, 5 * 60 * 1000);
    }

    #[test]
    fn a_growing_cluster_widens_rather_than_duplicating() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!("{}\n", token_count("2026-08-13T18:00:00.000Z", 100)),
        );
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", token_count("2026-08-13T18:04:00.000Z", 100)).unwrap();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        // Still one interval; the second pass extended it rather than adding a
        // second overlapping span.
        let (rows, started_at, ended_at): (i64, String, String) = conn
            .query_row(
                "SELECT count(*), MIN(started_at), MAX(ended_at) FROM telemetry_activity",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(started_at, "2026-08-13T18:00:00.000Z");
        assert_eq!(ended_at, "2026-08-13T18:04:00.000Z");
    }

    #[test]
    fn work_spanning_delta_boundaries_stays_one_interval() {
        // Each ingest cycle sees only new bytes. Without anchoring on the last
        // stored interval, a continuous work session would fragment into one
        // 40s singleton per cycle rather than a single growing span.
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!("{}\n", token_count("2026-08-13T18:00:00.000Z", 10)),
        );
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        for minute in ["18:03:00", "18:06:00", "18:09:00"] {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(
                file,
                "{}",
                token_count(&format!("2026-08-13T{minute}.000Z"), 10)
            )
            .unwrap();
            ingest_source(&conn, &codex_ctx(&path)).unwrap();
        }

        let (rows, ended_at, events): (i64, String, i64) = conn
            .query_row(
                "SELECT count(*), MAX(ended_at), MAX(event_count) FROM telemetry_activity",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(ended_at, "2026-08-13T18:09:00.000Z");
        assert_eq!(events, 4);

        let active: i64 = conn
            .query_row(
                "SELECT clustered_active_ms FROM telemetry_rollup_hourly",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active, 9 * 60 * 1000);
    }

    #[test]
    fn a_long_gap_starts_a_new_interval_across_deltas() {
        // The anchor must not glue together genuinely separate work sessions.
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!("{}\n", token_count("2026-08-13T18:00:00.000Z", 10)),
        );
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        // Well past the 12 minute threshold.
        writeln!(file, "{}", token_count("2026-08-13T18:40:00.000Z", 10)).unwrap();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let rows: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_activity", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 2);
    }

    #[test]
    fn resuming_after_a_long_gap_leaves_the_earlier_interval_untouched() {
        // The anchor is a synthetic event, and when nothing joins its cluster it
        // comes back out of the clusterer as a singleton ending one singleton
        // credit later than it started. Written back, that stretched the stored
        // interval by 40 seconds every time a session resumed after a break —
        // small per occurrence, and permanent.
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!("{}\n", token_count("2026-08-13T18:00:00.000Z", 10)),
        );
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let before: (String, String, i64) = conn
            .query_row(
                "SELECT started_at, ended_at, event_count FROM telemetry_activity",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", token_count("2026-08-13T18:40:00.000Z", 10)).unwrap();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let after: (String, String, i64) = conn
            .query_row(
                "SELECT started_at, ended_at, event_count FROM telemetry_activity
                 WHERE started_at = ?1",
                params![before.0],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(after, before);

        // And the total is two singleton credits, not three.
        let clustered: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(clustered_active_ms), 0) FROM telemetry_rollup_hourly",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(clustered, 2 * crate::telemetry::ACTIVE_SINGLETON_MS);
    }

    #[test]
    fn an_event_just_past_the_gap_threshold_splits_however_it_was_read() {
        // The anchor used to be the singleton's *credited* end, 40s after the
        // real event. An event 12m20s after the real event is only 11m40s after
        // that credited end, so it fell inside the 12m threshold and merged —
        // while a single full read split it. The same log then produced one
        // 12m20s interval or two 40s singletons depending purely on when ingest
        // ran.
        let gap = "18:12:20";

        // Incremental: two passes, the second landing just past the threshold.
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!("{}\n", token_count("2026-08-13T18:00:00.000Z", 10)),
        );
        let incremental = db();
        ingest_source(&incremental, &codex_ctx(&path)).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(
            file,
            "{}",
            token_count(&format!("2026-08-13T{gap}.000Z"), 10)
        )
        .unwrap();
        ingest_source(&incremental, &codex_ctx(&path)).unwrap();

        // Single pass over the identical bytes.
        let single_dir = tempfile::tempdir().unwrap();
        let single_path = write_log(
            single_dir.path(),
            &format!(
                "{}\n{}\n",
                token_count("2026-08-13T18:00:00.000Z", 10),
                token_count(&format!("2026-08-13T{gap}.000Z"), 10)
            ),
        );
        let single = db();
        ingest_source(&single, &codex_ctx(&single_path)).unwrap();

        let spans = |conn: &Connection| -> Vec<(String, String)> {
            let mut stmt = conn
                .prepare("SELECT started_at, ended_at FROM telemetry_activity ORDER BY started_at")
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };

        assert_eq!(spans(&incremental), spans(&single));
        // And the answer is the correct one: past the threshold, so two spans.
        assert_eq!(spans(&single).len(), 2);
    }

    #[test]
    fn a_parser_bump_replaces_facts_rather_than_colliding_with_them() {
        // The recovery path a version bump exists for. `INSERT OR IGNORE` cannot
        // correct an existing row, so without purging first the re-read collides
        // with exactly the bad facts it was meant to replace and the defect
        // survives.
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!(
                "{TURN_CONTEXT}\n{}\n",
                token_count("2026-08-13T18:42:49.628Z", 100)
            ),
        );
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        // Corrupt the stored fact the way a parser defect would have.
        conn.execute(
            "INSERT OR IGNORE INTO telemetry_strings(kind, value) VALUES ('model', 'wrong-model')",
            params![],
        )
        .unwrap();
        conn.execute(
            "UPDATE telemetry_turn_facts
             SET input_tokens = 999999,
                 model_ref = (SELECT string_id FROM telemetry_strings
                              WHERE kind = 'model' AND value = 'wrong-model')",
            params![],
        )
        .unwrap();
        conn.execute("UPDATE telemetry_sources SET parser_version = 0", params![])
            .unwrap();

        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let (turns, input, model): (i64, i64, Option<String>) = conn
            .query_row(
                "SELECT count(*), SUM(input_tokens), MIN(model) FROM telemetry_turns",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            turns, 1,
            "the corrected fact must not sit beside the old one"
        );
        assert_eq!(input, 100, "the re-read value must win");
        assert_eq!(model.as_deref(), Some("gpt-5.6-terra"));

        // And the rollup reflects the correction rather than the stale fact.
        let rolled: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(input_tokens), 0) FROM telemetry_rollup_hourly",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rolled, 100);
    }

    #[test]
    fn a_replacement_mid_first_line_does_not_resume_from_the_old_offset() {
        // A known identity that becomes unreadable means the file changed under
        // us while its first line was still being written. Treating that as "not
        // stale" would consume the replacement from the middle, losing its
        // opening records permanently.
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!(
                "{TURN_CONTEXT}\n{}\n",
                token_count("2026-08-13T18:00:00.000Z", 10)
            ),
        );
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        // Replacement whose first line has no terminator yet.
        std::fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"new",
        )
        .unwrap();
        let interrupted = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(
            interrupted.cursor_before, 0,
            "an unreadable identity is stale"
        );

        // The writer finishes the replacement.
        let replacement = format!(
            "{{\"timestamp\":\"2026-08-13T19:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"new\"}}}}\n{}\n",
            token_count("2026-08-13T19:00:00.000Z", 55)
        );
        std::fs::write(&path, &replacement).unwrap();
        let outcome = ingest_source(&conn, &codex_ctx(&path)).unwrap();

        assert_eq!(outcome.cursor_before, 0);
        assert_eq!(outcome.cursor_after, replacement.len() as i64);
        assert_eq!(outcome.turns, 1);
    }

    #[test]
    fn identical_records_at_different_positions_are_both_kept() {
        // Two byte-identical lines are two events. Keying on content alone would
        // silently merge them into one, so the key carries position too.
        let dir = tempfile::tempdir().unwrap();
        let repeated = token_count("2026-08-13T18:00:00.000Z", 0);
        let path = write_log(dir.path(), &format!("{repeated}\n{repeated}\n"));
        let conn = db();

        let outcome = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(outcome.turns, 2);

        let stored: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, 2);

        // Still idempotent: a full re-read collides with both.
        conn.execute("UPDATE telemetry_sources SET parser_version = 0", params![])
            .unwrap();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();
        let after: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(after, 2);
    }

    #[test]
    fn a_record_split_from_its_turn_context_still_ingests_once() {
        // Codex states turn identity in a separate record. An incremental read
        // can land between the two, so the same event parsed incrementally and
        // then re-read in full used to produce two rows: the dedup key included
        // the nullable turn id, and SQL compares NULLs as distinct.
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(dir.path(), &format!("{TURN_CONTEXT}\n"));
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        // The token count arrives in a later delta, with its context already
        // consumed by the previous pass.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", token_count("2026-08-13T18:42:49.628Z", 100)).unwrap();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        // Carried context means the split record is attributed exactly as a
        // single-pass read would have attributed it.
        let (turn_id, model): (Option<String>, Option<String>) = conn
            .query_row("SELECT turn_id, model FROM telemetry_turns", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(turn_id.as_deref(), Some("t-1"));
        assert_eq!(model.as_deref(), Some("gpt-5.6-terra"));

        // Now force the full re-read a parser fix would cause.
        conn.execute("UPDATE telemetry_sources SET parser_version = 0", params![])
            .unwrap();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let (turns, input): (i64, i64) = conn
            .query_row(
                "SELECT count(*), COALESCE(SUM(input_tokens), 0) FROM telemetry_turns",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(turns, 1);
        assert_eq!(input, 100);
    }

    #[test]
    fn parser_version_bump_forces_a_re_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(
            dir.path(),
            &format!(
                "{TURN_CONTEXT}\n{}\n",
                token_count("2026-08-13T18:42:49.628Z", 100)
            ),
        );
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        conn.execute("UPDATE telemetry_sources SET parser_version = 0", params![])
            .unwrap();

        let outcome = ingest_source(&conn, &codex_ctx(&path)).unwrap();
        assert_eq!(outcome.cursor_before, 0);
        assert_eq!(outcome.turns, 1);
        // Re-read, but the store did not double count.
        let turns: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(turns, 1);
    }

    #[test]
    fn a_missing_log_surfaces_as_unavailable() {
        let conn = db();
        let ctx = codex_ctx(std::path::Path::new("nowhere/rollout.jsonl"));
        let err = ingest_source(&conn, &ctx).unwrap_err();
        assert!(matches!(
            err,
            IngestError::Source(SourceError::Unavailable(_))
        ));
    }

    #[test]
    fn cumulative_gauge_is_never_summed_end_to_end() {
        // The log's total_token_usage says 999999 on every record; the ingested
        // total must reflect only the per-call deltas.
        let dir = tempfile::tempdir().unwrap();
        let contents = format!(
            "{TURN_CONTEXT}\n{}\n{}\n",
            token_count("2026-08-13T18:00:00.000Z", 100),
            token_count("2026-08-13T18:01:00.000Z", 250)
        );
        let path = write_log(dir.path(), &contents);
        let conn = db();
        ingest_source(&conn, &codex_ctx(&path)).unwrap();

        let total: i64 = conn
            .query_row(
                "SELECT input_tokens FROM telemetry_rollup_hourly",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(total, 350);
    }
}

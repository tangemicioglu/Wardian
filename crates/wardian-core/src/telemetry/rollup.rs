//! Hourly rollup recomputation.
//!
//! Surfaces read rollups, never facts. Hourly buckets collapse the volume that
//! made the prior forensic approach unusable: a single agent produced 38,931
//! timestamped events in one week, which becomes at most 168 rows here.
//!
//! Rollups are always derived, so recomputation is the repair mechanism for any
//! defect in this file. Recomputing a bucket deletes and rebuilds it, which
//! makes the operation idempotent and safe to run on overlapping dirty sets.

use crate::telemetry::activity::interval_duration_ms;
use crate::telemetry::models::{ActivityMethod, IntervalFact};
use crate::telemetry::store::{mark_dirty, mark_dirty_span, DirtyBuckets};
use rusqlite::{params, Connection};

/// Recompute every dirty `(bucket, session)` pair.
///
/// Takes `&Connection` rather than opening its own transaction so the caller can
/// commit this together with the fact write and cursor advance that produced the
/// dirty set. Committing separately would let a crash in between leave the
/// cursor past buckets that were never rebuilt.
pub(crate) fn recompute_buckets(conn: &Connection, dirty: &DirtyBuckets) -> rusqlite::Result<()> {
    for (bucket_start, session_id) in dirty {
        recompute_one(conn, bucket_start, session_id)?;
    }
    Ok(())
}

/// Rebuild every rollup bucket touched by either the existing derived rows or
/// the current canonical facts.
///
/// Attribution repair cannot know which individual buckets were made stale by
/// old writes, so it deliberately reconstructs the complete dirty set from
/// facts and rollups. The operation remains bounded by the stored history and
/// is idempotent: every selected bucket is deleted and derived again.
pub(crate) fn rebuild_all_rollups(conn: &Connection) -> rusqlite::Result<usize> {
    let mut dirty = DirtyBuckets::new();

    let mut rollups =
        conn.prepare("SELECT bucket_start, session_id FROM telemetry_rollup_hourly")?;
    for row in rollups.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))? {
        let (bucket, session): (String, String) = row?;
        dirty.insert((bucket, session));
    }

    let mut turns = conn.prepare("SELECT session_id, ended_at FROM telemetry_turns")?;
    for row in turns.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))? {
        let (session, ended_at): (String, String) = row?;
        mark_dirty(&mut dirty, &session, &ended_at);
    }

    let mut edits = conn.prepare("SELECT session_id, occurred_at FROM telemetry_edits")?;
    for row in edits.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))? {
        let (session, occurred_at): (String, String) = row?;
        mark_dirty(&mut dirty, &session, &occurred_at);
    }

    let mut activity =
        conn.prepare("SELECT session_id, started_at, ended_at FROM telemetry_activity")?;
    for row in activity.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (session, started_at, ended_at) = row?;
        mark_dirty_span(&mut dirty, &session, &started_at, &ended_at);
    }

    let count = dirty.len();
    recompute_buckets(conn, &dirty)?;
    Ok(count)
}

fn recompute_one(conn: &Connection, bucket_start: &str, session_id: &str) -> rusqlite::Result<()> {
    let bucket_end = next_hour(bucket_start);

    conn.execute(
        "DELETE FROM telemetry_rollup_hourly WHERE bucket_start = ?1 AND session_id = ?2",
        params![bucket_start, session_id],
    )?;

    // Turns and tokens, grouped by the model in force for each turn. Model is
    // per turn rather than per session because it changes mid-session.
    //
    // Every token sum is deliberately left un-COALESCEd. `SUM` over all-NULL
    // input yields NULL, which is exactly right: a component nothing reported
    // stays absent rather than becoming a zero that reads as "measured, and it
    // was none". `tokens_reported` then answers the coarser question of whether
    // this bucket carries token accounting at all, which is what separates a
    // provider reporting zero from antigravity reporting nothing.
    conn.execute(
        "INSERT INTO telemetry_rollup_hourly (
             bucket_start, session_id, provider, model, turns,
             input_tokens, cached_input_tokens, cache_write_tokens,
             output_tokens, reasoning_tokens, tokens_reported, cost_usd
         )
         SELECT ?1, session_id, provider, COALESCE(model, ''),
                -- Distinct turns, not model calls: codex emits one token_count
                -- record per call and several calls can belong to one turn, so
                -- COUNT(*) would report calls under a heading that says turns.
                --
                -- Known and structural: this distinctness is *per bucket and per
                -- model*, so summing it across a horizon slightly over-counts. A
                -- turn that crosses an hour boundary is counted in both buckets,
                -- and one that switches model mid-turn is counted on both rows.
                -- A global COUNT(DISTINCT ...) cannot be recovered from
                -- pre-aggregated per-bucket counts at all, which is the price of
                -- the rollup; the alternative that sums exactly is counting
                -- calls, and that answers a different question. The error is
                -- bounded by the number of boundaries each turn crosses, so it
                -- is small for turns measured in minutes.
                COUNT(DISTINCT COALESCE(turn_id, event_key)),
                SUM(input_tokens),
                SUM(cached_input_tokens),
                SUM(cache_write_tokens),
                SUM(output_tokens),
                SUM(reasoning_tokens),
                CASE WHEN COUNT(input_tokens) + COUNT(cached_input_tokens)
                        + COUNT(cache_write_tokens) + COUNT(output_tokens)
                        + COUNT(reasoning_tokens) > 0 THEN 1 ELSE 0 END,
                SUM(cost_usd)
         FROM telemetry_turns
         WHERE session_id = ?2
           AND julianday(ended_at) >= julianday(?1)
           AND julianday(ended_at) < julianday(?3)
         GROUP BY session_id, provider, COALESCE(model, '')",
        params![bucket_start, session_id, bucket_end],
    )?;

    // Activity is not attributable to a model, so it lands on the bucket's
    // primary model row (or a bare row when no turns were recorded).
    let (measured_ms, clustered_ms) =
        active_ms_in_bucket(conn, session_id, bucket_start, &bucket_end)?;
    let (files, added, removed) = edits_in_bucket(conn, session_id, bucket_start, &bucket_end)?;

    if measured_ms == 0 && clustered_ms == 0 && files == 0 && added == 0 && removed == 0 {
        return Ok(());
    }

    let provider = primary_provider(conn, session_id, bucket_start, &bucket_end)?;
    let model = primary_model(conn, bucket_start, session_id)?;

    // Stored apart, never added together. A single `active_ms` column would be
    // read as authoritative by everything downstream while silently containing
    // an estimate wherever a provider could not report real durations.
    let updated = conn.execute(
        "UPDATE telemetry_rollup_hourly
         SET measured_active_ms = ?1, clustered_active_ms = ?2,
             files_touched = ?3, lines_added = ?4, lines_removed = ?5
         WHERE bucket_start = ?6 AND session_id = ?7 AND model = ?8",
        params![
            measured_ms,
            clustered_ms,
            files,
            added,
            removed,
            bucket_start,
            session_id,
            model,
        ],
    )?;

    if updated == 0 {
        conn.execute(
            "INSERT INTO telemetry_rollup_hourly (
                 bucket_start, session_id, provider, model,
                 measured_active_ms, clustered_active_ms,
                 files_touched, lines_added, lines_removed
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                bucket_start,
                session_id,
                provider,
                model,
                measured_ms,
                clustered_ms,
                files,
                added,
                removed,
            ],
        )?;
    }

    Ok(())
}

/// Active milliseconds inside a bucket, split by method.
///
/// Intervals are clipped to the bucket, because a span that crosses an hour
/// boundary belongs partly to each. Measured and clustered totals are kept
/// apart: one is a fact and the other an estimate, and summing them without
/// saying so presents an inference as a measurement.
fn active_ms_in_bucket(
    conn: &Connection,
    session_id: &str,
    bucket_start: &str,
    bucket_end: &str,
) -> rusqlite::Result<(i64, i64)> {
    let mut stmt = conn.prepare(
        "SELECT started_at, ended_at, method
         FROM telemetry_activity
         WHERE session_id = ?1
           AND julianday(ended_at) > julianday(?2)
           AND julianday(started_at) < julianday(?3)",
    )?;
    let rows = stmt.query_map(params![session_id, bucket_start, bucket_end], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut measured = 0;
    let mut clustered = 0;
    for row in rows {
        let (started_at, ended_at, method) = row?;
        let clipped_end = min_timestamp(&ended_at, bucket_end);
        let clipped = IntervalFact {
            session_id: session_id.to_string(),
            provider: String::new(),
            started_at: max_timestamp(&started_at, bucket_start),
            // Duration is measured over the credited span, so the last real
            // event is irrelevant here.
            last_event_at: clipped_end.clone(),
            ended_at: clipped_end,
            event_count: 0,
            method: ActivityMethod::Clustered,
        };
        let ms = interval_duration_ms(&clipped);
        if ActivityMethod::parse(&method) == Some(ActivityMethod::Measured) {
            measured += ms;
        } else {
            clustered += ms;
        }
    }
    Ok((measured, clustered))
}

fn edits_in_bucket(
    conn: &Connection,
    session_id: &str,
    bucket_start: &str,
    bucket_end: &str,
) -> rusqlite::Result<(i64, i64, i64)> {
    conn.query_row(
        "SELECT COUNT(DISTINCT path),
                COALESCE(SUM(lines_added), 0),
                COALESCE(SUM(lines_removed), 0)
         FROM telemetry_edits
         WHERE session_id = ?1
           AND julianday(occurred_at) >= julianday(?2)
           AND julianday(occurred_at) < julianday(?3)",
        params![session_id, bucket_start, bucket_end],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
}

fn primary_provider(
    conn: &Connection,
    session_id: &str,
    bucket_start: &str,
    bucket_end: &str,
) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT provider FROM telemetry_activity
         WHERE session_id = ?1
           AND julianday(ended_at) > julianday(?2)
           AND julianday(started_at) < julianday(?3)
         LIMIT 1",
        params![session_id, bucket_start, bucket_end],
        |row| row.get(0),
    )
    .or_else(|_| {
        conn.query_row(
            "SELECT provider FROM telemetry_edits
             WHERE session_id = ?1
               AND julianday(occurred_at) >= julianday(?2)
               AND julianday(occurred_at) < julianday(?3)
             LIMIT 1",
            params![session_id, bucket_start, bucket_end],
            |row| row.get(0),
        )
    })
    .or_else(|_| Ok::<String, rusqlite::Error>(String::new()))
}

/// The model carrying the most tokens in the bucket, used as the row activity
/// attaches to. Empty when the bucket has no turns at all.
fn primary_model(
    conn: &Connection,
    bucket_start: &str,
    session_id: &str,
) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT model FROM telemetry_rollup_hourly
         WHERE bucket_start = ?1 AND session_id = ?2
         ORDER BY (input_tokens + output_tokens) DESC, model
         LIMIT 1",
        params![bucket_start, session_id],
        |row| row.get(0),
    )
    .or_else(|_| Ok::<String, rusqlite::Error>(String::new()))
}

fn max_timestamp(left: &str, right: &str) -> String {
    match (
        crate::telemetry::activity::parse_timestamp(left),
        crate::telemetry::activity::parse_timestamp(right),
    ) {
        (Some(left), Some(right)) if left >= right => {
            crate::telemetry::activity::format_timestamp(left)
        }
        (Some(_), Some(right)) => crate::telemetry::activity::format_timestamp(right),
        _ if left >= right => left.to_string(),
        _ => right.to_string(),
    }
}

fn min_timestamp(left: &str, right: &str) -> String {
    match (
        crate::telemetry::activity::parse_timestamp(left),
        crate::telemetry::activity::parse_timestamp(right),
    ) {
        (Some(left), Some(right)) if left <= right => {
            crate::telemetry::activity::format_timestamp(left)
        }
        (Some(_), Some(right)) => crate::telemetry::activity::format_timestamp(right),
        _ if left <= right => left.to_string(),
        _ => right.to_string(),
    }
}

/// Advance an hour-bucket timestamp by one hour.
pub fn next_hour(bucket_start: &str) -> String {
    match crate::telemetry::activity::parse_timestamp(bucket_start) {
        Some(parsed) => {
            crate::telemetry::activity::format_timestamp(parsed + chrono::Duration::hours(1))
        }
        None => bucket_start.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::models::{
        Cursor, CursorKind, EditFact, EditOp, ParsedFacts, SourceKind, TurnFact,
    };
    use crate::telemetry::schema::run_telemetry_migrations;
    use crate::telemetry::store::{write_facts, SourceState};

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        conn
    }

    fn state(provider: &str) -> SourceState {
        SourceState {
            source_key: crate::telemetry::store::source_key(provider, "agent-1", "src"),
            source_path: "src".into(),
            session_id: "agent-1".into(),
            provider_session_id: None,
            provider: provider.into(),
            source_kind: SourceKind::Jsonl,
            cursor: Cursor::new(CursorKind::ByteOffset, 0),
            last_size: 0,
            last_modified: None,
            parser_version: 1,
            fingerprint: None,
            carry: Default::default(),
        }
    }

    fn turn(turn_id: &str, ended_at: &str, model: &str, input: Option<i64>) -> TurnFact {
        TurnFact {
            event_key: format!("{turn_id}:{ended_at}"),
            session_id: "agent-1".into(),
            provider: "codex".into(),
            turn_id: Some(turn_id.into()),
            model: Some(model.into()),
            effort: None,
            started_at: None,
            ended_at: ended_at.into(),
            input_tokens: input,
            cached_input_tokens: input.map(|_| 50),
            cache_write_tokens: input.map(|_| 0),
            output_tokens: input.map(|_| 10),
            reasoning_tokens: input.map(|_| 5),
            context_window: None,
            cost_usd: None,
        }
    }

    fn ingest(conn: &Connection, facts: ParsedFacts, intervals: Vec<IntervalFact>) {
        let dirty = write_facts(conn, &facts, &intervals, &state("codex")).unwrap();
        recompute_buckets(conn, &dirty).unwrap();
    }

    /// `(model, turns, input_tokens, cached_input_tokens, total_active_ms)`.
    type RollupProbe = (String, i64, Option<i64>, Option<i64>, i64);

    fn rollup(conn: &Connection, bucket: &str) -> Vec<RollupProbe> {
        let mut stmt = conn
            .prepare(
                "SELECT model, turns, input_tokens, cached_input_tokens,
                        measured_active_ms + clustered_active_ms
                 FROM telemetry_rollup_hourly WHERE bucket_start = ?1 ORDER BY model",
            )
            .unwrap();
        stmt.query_map(params![bucket], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    }

    #[test]
    fn tokens_aggregate_into_the_hour_bucket() {
        let conn = db();
        ingest(
            &conn,
            ParsedFacts {
                turns: vec![
                    turn("t1", "2026-08-13T18:10:00.000Z", "gpt-5.6", Some(100)),
                    turn("t2", "2026-08-13T18:50:00.000Z", "gpt-5.6", Some(200)),
                ],
                ..Default::default()
            },
            vec![],
        );
        let rows = rollup(&conn, "2026-08-13T18:00:00.000Z");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, 2); // turns
        assert_eq!(rows[0].2, Some(300)); // input tokens
    }

    #[test]
    fn cached_tokens_stay_in_their_own_column() {
        let conn = db();
        ingest(
            &conn,
            ParsedFacts {
                turns: vec![turn("t1", "2026-08-13T18:10:00.000Z", "gpt-5.6", Some(100))],
                ..Default::default()
            },
            vec![],
        );
        let rows = rollup(&conn, "2026-08-13T18:00:00.000Z");
        assert_eq!(rows[0].2, Some(100));
        assert_eq!(rows[0].3, Some(50));
    }

    #[test]
    fn models_are_separate_rows_within_a_bucket() {
        let conn = db();
        ingest(
            &conn,
            ParsedFacts {
                turns: vec![
                    turn("t1", "2026-08-13T18:10:00.000Z", "gpt-5.6", Some(100)),
                    turn("t2", "2026-08-13T18:20:00.000Z", "gpt-5.4", Some(70)),
                ],
                ..Default::default()
            },
            vec![],
        );
        let rows = rollup(&conn, "2026-08-13T18:00:00.000Z");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn multiple_token_counts_in_one_turn_count_as_one_turn() {
        // Codex emits several token_count records per user turn; the rollup
        // counts distinct turn ids, not model calls.
        let conn = db();
        ingest(
            &conn,
            ParsedFacts {
                turns: vec![
                    turn("t1", "2026-08-13T18:10:00.000Z", "gpt-5.6", Some(100)),
                    turn("t1", "2026-08-13T18:11:00.000Z", "gpt-5.6", Some(150)),
                ],
                ..Default::default()
            },
            vec![],
        );
        let rows = rollup(&conn, "2026-08-13T18:00:00.000Z");
        assert_eq!(rows[0].1, 1); // one turn
        assert_eq!(rows[0].2, Some(250)); // but both token deltas
    }

    #[test]
    fn missing_tokens_are_flagged_as_unreported() {
        let conn = db();
        ingest(
            &conn,
            ParsedFacts {
                turns: vec![turn("t1", "2026-08-13T18:10:00.000Z", "unknown", None)],
                ..Default::default()
            },
            vec![],
        );
        let (reported, input): (i64, Option<i64>) = conn
            .query_row(
                "SELECT tokens_reported, input_tokens FROM telemetry_rollup_hourly",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(reported, 0);
        // Not zero: a provider that reports nothing must not read as one that
        // reported none, or it ranks as the cheapest.
        assert_eq!(input, None);
    }

    #[test]
    fn a_partially_reported_turn_keeps_its_gaps_null() {
        // The trap this guards: coalescing every component to zero makes a
        // provider that reports input but not reasoning indistinguishable from
        // one that genuinely did no reasoning.
        let conn = db();
        let partial = TurnFact {
            cached_input_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            ..turn("t1", "2026-08-13T18:10:00.000Z", "gpt-5.6", Some(100))
        };
        ingest(
            &conn,
            ParsedFacts {
                turns: vec![partial],
                ..Default::default()
            },
            vec![],
        );

        let (input, cached, reasoning, reported): (Option<i64>, Option<i64>, Option<i64>, i64) =
            conn.query_row(
                "SELECT input_tokens, cached_input_tokens, reasoning_tokens, tokens_reported
                 FROM telemetry_rollup_hourly",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(input, Some(100));
        assert_eq!(cached, None);
        assert_eq!(reasoning, None);
        assert_eq!(reported, 1);
    }

    #[test]
    fn measured_and_clustered_activity_are_tracked_apart() {
        let conn = db();
        let intervals = vec![
            IntervalFact {
                session_id: "agent-1".into(),
                provider: "opencode".into(),
                started_at: "2026-08-13T18:00:00.000Z".into(),
                ended_at: "2026-08-13T18:10:00.000Z".into(),
                last_event_at: "2026-08-13T18:10:00.000Z".into(),
                event_count: 1,
                method: ActivityMethod::Measured,
            },
            IntervalFact {
                session_id: "agent-1".into(),
                provider: "codex".into(),
                started_at: "2026-08-13T18:20:00.000Z".into(),
                ended_at: "2026-08-13T18:25:00.000Z".into(),
                last_event_at: "2026-08-13T18:25:00.000Z".into(),
                event_count: 3,
                method: ActivityMethod::Clustered,
            },
        ];
        ingest(&conn, ParsedFacts::default(), intervals);

        let (measured, clustered): (i64, i64) = conn
            .query_row(
                "SELECT measured_active_ms, clustered_active_ms FROM telemetry_rollup_hourly",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(measured, 10 * 60 * 1000);
        assert_eq!(clustered, 5 * 60 * 1000);

        // No blended column exists to be mistaken for a measurement.
        let blended = conn.query_row("SELECT active_ms FROM telemetry_rollup_hourly", [], |row| {
            row.get::<_, i64>(0)
        });
        assert!(blended.is_err());
    }

    #[test]
    fn an_interval_crossing_an_hour_is_split_between_buckets() {
        let conn = db();
        ingest(
            &conn,
            ParsedFacts::default(),
            vec![IntervalFact {
                session_id: "agent-1".into(),
                provider: "codex".into(),
                started_at: "2026-08-13T18:50:00.000Z".into(),
                ended_at: "2026-08-13T19:10:00.000Z".into(),
                last_event_at: "2026-08-13T19:10:00.000Z".into(),
                event_count: 5,
                method: ActivityMethod::Clustered,
            }],
        );
        // The write marks only the start bucket dirty, so recompute the next
        // one explicitly as the ingest loop does for a spanning interval.
        let mut dirty = DirtyBuckets::new();
        dirty.insert((
            "2026-08-13T19:00:00.000Z".to_string(),
            "agent-1".to_string(),
        ));
        recompute_buckets(&conn, &dirty).unwrap();

        let first = rollup(&conn, "2026-08-13T18:00:00.000Z")[0].4;
        let second = rollup(&conn, "2026-08-13T19:00:00.000Z")[0].4;
        assert_eq!(first, 10 * 60 * 1000);
        assert_eq!(second, 10 * 60 * 1000);
    }

    #[test]
    fn edits_aggregate_files_and_lines() {
        let conn = db();
        ingest(
            &conn,
            ParsedFacts {
                edits: vec![
                    EditFact {
                        event_key: "edit-1".into(),
                        session_id: "agent-1".into(),
                        provider: "codex".into(),
                        turn_id: None,
                        occurred_at: "2026-08-13T18:10:00.000Z".into(),
                        workspace: None,
                        path: "a.md".into(),
                        op: EditOp::Add,
                        lines_added: Some(10),
                        lines_removed: Some(0),
                    },
                    EditFact {
                        event_key: "edit-2".into(),
                        session_id: "agent-1".into(),
                        provider: "codex".into(),
                        turn_id: None,
                        occurred_at: "2026-08-13T18:20:00.000Z".into(),
                        workspace: None,
                        path: "b.md".into(),
                        op: EditOp::Update,
                        lines_added: Some(3),
                        lines_removed: Some(2),
                    },
                ],
                ..Default::default()
            },
            vec![],
        );
        let (files, added, removed): (i64, i64, i64) = conn
            .query_row(
                "SELECT files_touched, lines_added, lines_removed FROM telemetry_rollup_hourly",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((files, added, removed), (2, 13, 2));
    }

    #[test]
    fn the_same_file_edited_twice_counts_once() {
        let conn = db();
        ingest(
            &conn,
            ParsedFacts {
                edits: vec![
                    EditFact {
                        event_key: "edit-3".into(),
                        session_id: "agent-1".into(),
                        provider: "codex".into(),
                        turn_id: None,
                        occurred_at: "2026-08-13T18:10:00.000Z".into(),
                        workspace: None,
                        path: "a.md".into(),
                        op: EditOp::Update,
                        lines_added: Some(1),
                        lines_removed: Some(1),
                    },
                    EditFact {
                        event_key: "edit-4".into(),
                        session_id: "agent-1".into(),
                        provider: "codex".into(),
                        turn_id: None,
                        occurred_at: "2026-08-13T18:30:00.000Z".into(),
                        workspace: None,
                        path: "a.md".into(),
                        op: EditOp::Update,
                        lines_added: Some(2),
                        lines_removed: Some(0),
                    },
                ],
                ..Default::default()
            },
            vec![],
        );
        let files: i64 = conn
            .query_row(
                "SELECT files_touched FROM telemetry_rollup_hourly",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(files, 1);
    }

    #[test]
    fn recomputation_is_idempotent() {
        let conn = db();
        let facts = ParsedFacts {
            turns: vec![turn("t1", "2026-08-13T18:10:00.000Z", "gpt-5.6", Some(100))],
            ..Default::default()
        };
        ingest(&conn, facts.clone(), vec![]);
        let before = rollup(&conn, "2026-08-13T18:00:00.000Z");

        let mut dirty = DirtyBuckets::new();
        dirty.insert((
            "2026-08-13T18:00:00.000Z".to_string(),
            "agent-1".to_string(),
        ));
        recompute_buckets(&conn, &dirty).unwrap();
        recompute_buckets(&conn, &dirty).unwrap();

        assert_eq!(before, rollup(&conn, "2026-08-13T18:00:00.000Z"));
    }

    #[test]
    fn next_hour_advances() {
        assert_eq!(
            next_hour("2026-08-13T23:00:00.000Z"),
            "2026-08-14T00:00:00.000Z"
        );
    }
}

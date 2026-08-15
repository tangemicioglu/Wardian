//! The whole chain, on the committed fixture.
//!
//! Every other test covers one link: parsing, fact writing, rollup arithmetic,
//! or a query against hand-seeded rows. None covers all of them at once, and the
//! links are where the interesting failures live — a rollup that sums a column
//! the writer never populated is invisible to both sides' own tests and shows up
//! as a Dashboard reading zero while the facts underneath are correct.
//!
//! So this ingests a real log shape and then asks the read layer the questions
//! the Dashboard actually asks, with no hand-seeded rows anywhere in between.

use rusqlite::Connection;
use wardian_core::telemetry::horizon::{resolve_horizon, Horizon};
use wardian_core::telemetry::ingest::ingest_source;
use wardian_core::telemetry::query::{
    activity_intervals, breakdown, latest_limits, series, summary, Dimension,
};
use wardian_core::telemetry::schema::run_telemetry_migrations;
use wardian_core::telemetry::sources::SourceContext;

fn fixture_log() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("codex-rollout.jsonl")
}

/// A window wide enough to hold the fixture, which is timestamped in the past.
const FROM: &str = "2020-01-01T00:00:00.000Z";
const TO: &str = "2100-01-01T00:00:00.000Z";

fn ingested() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&conn).unwrap();
    let ctx = SourceContext::new("agent-chain", "codex", &fixture_log());
    let outcome = ingest_source(&conn, &ctx).unwrap();
    assert!(
        outcome.turns > 0,
        "fixture must produce turns, or every assertion below is vacuous"
    );
    conn
}

#[test]
fn the_summary_a_dashboard_asks_for_reaches_the_facts_that_were_ingested() {
    let conn = ingested();
    let totals = summary(&conn, FROM, TO).unwrap();

    // The same figures the token-invariant test pins against the log, arriving
    // here through the rollup rather than out of the fact table directly.
    // Input is fresh-only: 831,424 prompt tokens minus 730,880 cache reads.
    assert_eq!(totals.tokens.input_tokens, Some(100_544));
    assert_eq!(totals.tokens.cached_input_tokens, Some(730_880));
    assert_eq!(totals.tokens.output_tokens, Some(5_254));
    assert_eq!(totals.tokens.reasoning_tokens, Some(2_244));

    // New content processed. The pre-normalization store reported 836,678 for
    // this session — every cache read counted a second time.
    assert_eq!(totals.tokens.billable_total(), Some(105_798));

    // Two real turns across four token_count records: codex emits one record
    // per model call, so this counts distinct `turn_id`, not records.
    assert_eq!(totals.turns, 2);
    assert_eq!(totals.agent_count, 1);
    assert_eq!(totals.files_touched, 2);
    assert_eq!(totals.lines_added, 5);
    assert_eq!(totals.lines_removed, 1);
}

#[test]
fn codex_activity_arrives_as_an_estimate_rather_than_a_measurement() {
    // Codex reports no turn durations, so its active time is inferred by gap
    // clustering. Presenting that as `measured_ms` would let the Dashboard show
    // an upper bound as if the provider had stated it.
    let conn = ingested();
    let totals = summary(&conn, FROM, TO).unwrap();

    assert_eq!(totals.active.measured_ms, 0);
    assert!(totals.active.clustered_ms > 0);
    assert!(!totals.active.is_mixed());

    let intervals = activity_intervals(&conn, FROM, TO).unwrap();
    assert!(!intervals.is_empty());
    assert!(intervals
        .iter()
        .all(|interval| interval.method == wardian_core::telemetry::ActivityMethod::Clustered));
    // Every stored span must run forwards, or the timeline draws it backwards.
    assert!(intervals
        .iter()
        .all(|interval| interval.ended_at >= interval.started_at));
}

#[test]
fn every_breakdown_dimension_agrees_with_the_summary_it_came_from() {
    // A dimension that grouped on the wrong column, or dropped rows with a null
    // key, would still return plausible-looking rows. Reconciling each against
    // the summary is what makes that detectable.
    let conn = ingested();
    let totals = summary(&conn, FROM, TO).unwrap();

    for dimension in [Dimension::Provider, Dimension::Agent, Dimension::Model] {
        let rows = breakdown(&conn, dimension, FROM, TO, 24).unwrap();
        assert!(!rows.is_empty(), "{dimension:?} produced no rows");

        let turns: i64 = rows.iter().map(|row| row.turns).sum();
        let input: i64 = rows
            .iter()
            .filter_map(|row| row.tokens.input_tokens)
            .sum();

        assert_eq!(turns, totals.turns, "{dimension:?} turns disagree");
        assert_eq!(
            input,
            totals.tokens.input_tokens.unwrap(),
            "{dimension:?} input tokens disagree"
        );
    }
}

#[test]
fn distinct_counts_survive_an_hour_boundary() {
    // `turn-002` has calls at 14:55 and 15:02, so it falls in two hourly
    // buckets. Reading these two measures off the rollup counted it once per
    // bucket — distinctness does not survive pre-aggregation, and no
    // combination of per-bucket distinct counts recovers the global one. On a
    // real habitat that reported 1,813 files touched where 967 were.
    //
    // Both are now answered from the facts for the window, so the boundary is
    // invisible where it should be.
    let conn = ingested();
    let totals = summary(&conn, FROM, TO).unwrap();

    let distinct_turns: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT turn_id) FROM telemetry_turns WHERE turn_id IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let distinct_files: i64 = conn
        .query_row("SELECT COUNT(DISTINCT path) FROM telemetry_edits", [], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(distinct_turns, 2, "the fixture holds two real turns");
    assert_eq!(totals.turns, distinct_turns);
    assert_eq!(totals.files_touched, distinct_files);

    // And the rollup still holds the inflated figures, which is why the read
    // path must not go near them for these two measures.
    let rolled_up_turns: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(turns), 0) FROM telemetry_rollup_hourly",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        rolled_up_turns > distinct_turns,
        "the per-bucket rollup should still over-count, or this test proves nothing"
    );
}

#[test]
fn the_series_and_the_summary_report_the_same_billable_total() {
    // These are two implementations of one definition — one in SQL, one in
    // Rust. They have diverged before, and a divergence shows up as a chart
    // that disagrees with the tile directly above it.
    let conn = ingested();
    let totals = summary(&conn, FROM, TO).unwrap();
    let points = series(&conn, Dimension::Provider, FROM, TO).unwrap();

    let charted: i64 = points
        .iter()
        .filter_map(|point| point.billable_tokens)
        .sum();
    assert_eq!(Some(charted), totals.tokens.billable_total());
}

#[test]
fn rate_limits_survive_ingest_as_readings_rather_than_totals() {
    let conn = ingested();
    let limits = latest_limits(&conn).unwrap();

    // Four observations of one account collapse to the newest reading, not to
    // their sum — which would report 86% usage for an account at 32%.
    assert_eq!(limits.len(), 1);
    assert_eq!(limits[0].provider, "codex");
    assert!(limits[0].used_percent.unwrap() <= 100.0);
}

#[test]
fn a_horizon_that_excludes_the_fixture_reports_nothing_rather_than_failing() {
    // The empty-habitat path the Dashboard hits on a fresh install. It has to
    // be zeroes and nulls, not an error, and not a row of zeroed tokens.
    let conn = ingested();
    let window = resolve_horizon(Horizon::Today, chrono::Utc::now());
    let totals = summary(&conn, &window.from, &window.to).unwrap();

    assert_eq!(totals.turns, 0);
    assert_eq!(totals.agent_count, 0);
    assert_eq!(totals.active.measured_ms, 0);
    // No turns is not a report of zero tokens.
    assert_eq!(totals.tokens.input_tokens, None);
    assert!(!totals.tokens.any_reported());
    assert!(breakdown(&conn, Dimension::Provider, &window.from, &window.to, 24)
        .unwrap()
        .is_empty());
}

#[test]
fn re_ingesting_changes_no_answer() {
    // The property the whole store depends on: a scheduled pass runs every
    // minute over sources that mostly have not changed, and each one must be a
    // no-op rather than a re-count.
    let conn = ingested();
    let before = summary(&conn, FROM, TO).unwrap();

    let ctx = SourceContext::new("agent-chain", "codex", &fixture_log());
    // Force a full re-read the way a parser version bump would.
    conn.execute("UPDATE telemetry_sources SET parser_version = 0", [])
        .unwrap();
    ingest_source(&conn, &ctx).unwrap();

    assert_eq!(summary(&conn, FROM, TO).unwrap(), before);
}

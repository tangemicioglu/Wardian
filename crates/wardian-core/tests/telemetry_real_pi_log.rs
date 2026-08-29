//! Pi's token invariant, checked against a fixture and against a real log.
//!
//! Pi is the only provider that reports every component Wardian stores *and*
//! a real per-component cost, which makes one property load-bearing: its
//! `input` is cache-exclusive. `totalTokens` reconciles as
//! `input + output + cacheRead`, so cache reads sit outside `input` and there
//! is nothing to subtract. Applying codex's correction here would drive most
//! turns to zero; failing to apply codex's correction *there* overstated a real
//! habitat 49-fold. Both directions are silent, so both are asserted.
//!
//! **The committed fixture always runs.** It is transcribed from real record
//! shapes and carries the token figures observed on a real session, so it
//! proves the arithmetic and the ingest wiring on every machine including CI.
//! It cannot prove format fidelity, because it is something this crate wrote.
//!
//! **The real-log test runs when a pi session log is present**, and is the only
//! check that the format assumption still holds against a pi Wardian does not
//! control. It skips elsewhere, which is why it is not the only coverage.

use rusqlite::Connection;
use wardian_core::telemetry::ingest::ingest_source;
use wardian_core::telemetry::schema::run_telemetry_migrations;
use wardian_core::telemetry::sources::SourceContext;

/// The committed fixture, which every machine can run.
fn fixture_log() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("pi-session.jsonl")
}

/// Newest pi session log on this machine, if any.
///
/// Pi writes into a Wardian-owned directory per agent rather than a shared
/// provider home, so the search is over `~/.wardian/agents/*/pi/sessions`.
fn newest_pi_log() -> Option<std::path::PathBuf> {
    if let Ok(explicit) = std::env::var("WARDIAN_TEST_PI_LOG") {
        let path = std::path::PathBuf::from(explicit);
        return path.exists().then_some(path);
    }

    let root = std::env::var("WARDIAN_HOME")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|home| home.join(".wardian")))?
        .join("agents");

    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for agent in std::fs::read_dir(&root).ok()?.flatten() {
        let sessions = agent.path().join("pi").join("sessions");
        let Ok(entries) = std::fs::read_dir(&sessions) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
                continue;
            };
            if newest.as_ref().is_none_or(|(seen, _)| modified > *seen) {
                newest = Some((modified, path));
            }
        }
    }
    newest.map(|(_, path)| path)
}

fn ingest(path: &std::path::Path) -> Connection {
    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();
    let ctx = SourceContext::new("agent-real", "pi", path);
    ingest_source(&store, &ctx).unwrap();
    store
}

/// `(input, cached, cache_write, output, reasoning)` as stored.
fn ingested_totals(store: &Connection) -> (i64, i64, i64, i64, i64) {
    store
        .query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(cached_input_tokens),0),
                    COALESCE(SUM(cache_write_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(reasoning_tokens),0)
             FROM telemetry_turns",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap()
}

/// Every assistant message's `usage`, straight from the log.
fn declared_usage(path: &std::path::Path) -> Vec<serde_json::Value> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| record.get("type").and_then(|v| v.as_str()) == Some("message"))
        .filter_map(|record| record.get("message").cloned())
        .filter(|message| message.get("role").and_then(|v| v.as_str()) == Some("assistant"))
        .filter_map(|message| message.get("usage").cloned())
        .collect()
}

#[test]
fn the_fixture_ingests_every_component_pi_reports() {
    let store = ingest(&fixture_log());
    let (input, cached, cache_write, output, reasoning) = ingested_totals(&store);

    assert_eq!(input, 20_640);
    assert_eq!(cached, 7_680);
    assert_eq!(cache_write, 0);
    assert_eq!(output, 183);
    assert_eq!(reasoning, 98);
}

#[test]
fn the_fixture_stores_input_with_cache_reads_left_alone() {
    // The claim this file exists to defend. Pi's own `totalTokens` reconciles as
    // `input + output + cacheRead`, so `input` already excludes what came from
    // cache. Subtracting here — the adjustment codex needs — would store 12,960
    // where 20,640 tokens were really read fresh.
    let usage = declared_usage(&fixture_log());
    assert!(!usage.is_empty(), "fixture must carry assistant usage");

    for entry in &usage {
        let field = |key: &str| entry.get(key).and_then(serde_json::Value::as_i64).unwrap();
        assert_eq!(
            field("totalTokens"),
            field("input") + field("output") + field("cacheRead"),
            "pi usage must stay disjoint: {entry}"
        );
    }

    let declared_input: i64 = usage
        .iter()
        .filter_map(|entry| entry.get("input").and_then(serde_json::Value::as_i64))
        .sum();
    let (input, cached, ..) = ingested_totals(&ingest(&fixture_log()));
    assert_eq!(input, declared_input);
    assert!(cached > 0, "fixture must exercise a cached turn");
}

#[test]
fn the_fixture_keeps_reasoning_inside_output() {
    // Upstream documents `reasoning` as a subset of `output`. Nothing may add
    // the two, so the reconciliation above must hold *without* reasoning in it.
    let usage = declared_usage(&fixture_log());
    let reasoning: i64 = usage
        .iter()
        .filter_map(|entry| entry.get("reasoning").and_then(serde_json::Value::as_i64))
        .sum();
    assert!(reasoning > 0, "fixture must exercise reported reasoning");

    let (_, _, _, output, stored_reasoning) = ingested_totals(&ingest(&fixture_log()));
    assert_eq!(stored_reasoning, reasoning);
    assert!(
        stored_reasoning < output,
        "reasoning {stored_reasoning} must be a subset of output {output}"
    );
}

#[test]
fn the_fixture_costs_reconcile_to_their_components() {
    // What a future price table rests on: pi prices each component separately
    // and its total is exactly their sum, so per-token rates can be recovered by
    // division rather than transcribed from a vendor page.
    for entry in declared_usage(&fixture_log()) {
        let cost = entry.get("cost").expect("pi reports cost");
        let part = |key: &str| cost.get(key).and_then(serde_json::Value::as_f64).unwrap();
        let summed = part("input") + part("output") + part("cacheRead") + part("cacheWrite");
        assert!(
            (summed - part("total")).abs() < 1e-12,
            "cost components must sum to the total: {cost}"
        );
    }
}

#[test]
fn the_fixture_records_the_files_pi_wrote() {
    let store = ingest(&fixture_log());
    let (edits, added, removed): (i64, i64, i64) = store
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(lines_added),0), COALESCE(SUM(lines_removed),0)
             FROM telemetry_edits",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    // One `write` of a single line, and one `edit` replacing two lines with one.
    assert_eq!(edits, 2);
    assert_eq!(added, 2);
    assert_eq!(removed, 2);
}

#[test]
fn a_real_pi_log_still_reports_usage_the_way_this_parser_reads_it() {
    let Some(path) = newest_pi_log() else {
        eprintln!("no pi session log on this machine; skipping");
        return;
    };

    let usage = declared_usage(&path);
    if usage.is_empty() {
        eprintln!(
            "real pi log {} carries no usage yet; skipping",
            path.display()
        );
        return;
    }

    for entry in &usage {
        let field = |key: &str| entry.get(key).and_then(serde_json::Value::as_i64);
        let (Some(total), Some(input), Some(output), Some(cache_read)) = (
            field("totalTokens"),
            field("input"),
            field("output"),
            field("cacheRead"),
        ) else {
            panic!(
                "real pi log {} changed its usage shape: {entry}",
                path.display()
            );
        };
        assert_eq!(
            total,
            input + output + cache_read,
            "real pi log {}: usage stopped being disjoint, so `input` may now \
             include cache reads and this parser would double count them",
            path.display()
        );
        // Guards the same gap codex has: every observed write is 0 on an
        // OpenAI-backed route, so whether a nonzero write is additive to
        // `totalTokens` is untestable. Fail loudly the first time one appears
        // rather than fold an unverified quantity into a total.
        assert_eq!(
            field("cacheWrite"),
            Some(0),
            "real pi log {}: first nonzero cacheWrite observed — confirm whether \
             it is additive to totalTokens before trusting it in a total",
            path.display()
        );
    }

    let store = ingest(&path);
    let (input, cached, ..) = ingested_totals(&store);
    let declared_input: i64 = usage
        .iter()
        .filter_map(|entry| entry.get("input").and_then(serde_json::Value::as_i64))
        .sum();
    assert_eq!(input, declared_input, "real pi log {}", path.display());
    eprintln!(
        "real pi log {}: input={input} cached={cached}",
        path.display()
    );
}

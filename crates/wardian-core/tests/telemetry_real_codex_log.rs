//! Codex's token invariant, checked two ways.
//!
//! Codex reports both a per-call delta (`last_token_usage`) and a
//! session-cumulative gauge (`total_token_usage`). Summing the deltas must
//! reproduce the final gauge. Getting this wrong is silent — the figures stay
//! plausible while being wrong by roughly the call count — so it is worth more
//! than one kind of evidence.
//!
//! **The committed fixture always runs.** It is transcribed from real record
//! shapes and carries the token figures observed on a real session, so it proves
//! the arithmetic and the parser wiring on every machine including CI. It cannot
//! prove format fidelity, because it is something this crate wrote.
//!
//! **The real-log tests run when a log is present**, and are the only check that
//! the format assumption still holds against a codex Wardian does not control.
//! They skip elsewhere, which is why they are not the only coverage.

use rusqlite::Connection;
use wardian_core::telemetry::ingest::ingest_source;
use wardian_core::telemetry::schema::run_telemetry_migrations;
use wardian_core::telemetry::sources::SourceContext;

/// The committed fixture, which every machine can run.
fn fixture_log() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("codex-rollout.jsonl")
}

/// Newest codex rollout log on this machine, if any.
fn newest_codex_log() -> Option<std::path::PathBuf> {
    if let Ok(explicit) = std::env::var("WARDIAN_TEST_CODEX_LOG") {
        let path = std::path::PathBuf::from(explicit);
        return path.exists().then_some(path);
    }

    let root = dirs::home_dir()?.join(".codex").join("sessions");
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
                continue;
            };
            if newest
                .as_ref()
                .is_none_or(|(seen, _)| modified > *seen)
            {
                newest = Some((modified, path));
            }
        }
    }
    newest.map(|(_, path)| path)
}

/// The provider's own final cumulative figures, read straight from the log.
fn declared_totals(path: &std::path::Path) -> Option<(i64, i64, i64, i64)> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut latest = None;
    for line in content.lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let payload = record.get("payload")?;
        if payload.get("type").and_then(|value| value.as_str()) != Some("token_count") {
            continue;
        }
        let total = payload.get("info")?.get("total_token_usage")?;
        latest = Some((
            total.get("input_tokens")?.as_i64()?,
            total.get("cached_input_tokens")?.as_i64()?,
            total.get("output_tokens")?.as_i64()?,
            total.get("reasoning_output_tokens")?.as_i64()?,
        ));
    }
    latest
}

fn ingest(path: &std::path::Path) -> Connection {
    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();
    let ctx = SourceContext::new("agent-real", "codex", path);
    ingest_source(&store, &ctx).unwrap();
    store
}

fn ingested_totals(store: &Connection) -> (i64, i64, i64, i64) {
    store
        .query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(cached_input_tokens),0),
                    COALESCE(SUM(output_tokens),0), COALESCE(SUM(reasoning_tokens),0)
             FROM telemetry_turns",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
}

#[test]
fn the_fixture_summed_deltas_reproduce_its_cumulative_gauge() {
    // Runs everywhere, so the invariant is never silently unchecked.
    //
    // Stated as `fresh + cached == declared input`, not `input == declared
    // input`, because the store normalizes codex's prompt total into its
    // cache-exclusive part. The provider's own gauge is still fully accounted
    // for — it is just split across two columns that mean distinct things.
    let path = fixture_log();
    let declared = declared_totals(&path).expect("fixture must carry token_count records");
    let store = ingest(&path);
    let (input, cached, output, reasoning) = ingested_totals(&store);

    assert_eq!(declared, (831_424, 730_880, 5_254, 2_244));
    assert_eq!(input + cached, declared.0, "no prompt tokens went missing");
    assert_eq!(cached, declared.1);
    assert_eq!(output, declared.2);
    assert_eq!(reasoning, declared.3);
}

#[test]
fn the_fixture_stores_input_with_cache_reads_already_removed() {
    // The 49x bug, pinned. Of 831,424 prompt tokens this session billed, only
    // 100,544 were new content; the rest were cache hits. Storing the raw
    // figure made `input + output` count those hits twice, and every resulting
    // number stayed plausible while being wrong by more than an order of
    // magnitude.
    let store = ingest(&fixture_log());
    let (input, cached, ..) = ingested_totals(&store);

    assert_eq!(input, 100_544, "input must be fresh tokens only");
    assert_eq!(cached, 730_880);
    // Cache reads ran about 7x fresh input here, which is why folding the two
    // together is not a rounding error.
    assert!(cached / input >= 7);
}

#[test]
fn the_fixture_attributes_turns_and_models() {
    let store = ingest(&fixture_log());

    // Two `turn_context` records, and the token counts between them inherit
    // whichever was most recent.
    let turns: Vec<(Option<String>, Option<String>)> = {
        let mut stmt = store
            .prepare("SELECT turn_id, model FROM telemetry_turns ORDER BY ended_at")
            .unwrap();
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    assert_eq!(turns.len(), 4);
    assert_eq!(turns[0].0.as_deref(), Some("turn-001"));
    assert_eq!(turns[3].0.as_deref(), Some("turn-002"));
    assert!(turns.iter().all(|(_, model)| model.as_deref() == Some("gpt-5.6-terra")));
}

#[test]
fn the_fixture_counts_only_patches_that_applied() {
    let store = ingest(&fixture_log());

    // One successful patch touching two files; one failed patch touching none,
    // because it changed nothing on disk.
    let paths: Vec<String> = {
        let mut stmt = store
            .prepare("SELECT path FROM telemetry_edits ORDER BY path")
            .unwrap();
        let rows = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().all(|path| !path.ends_with("never.rs")));

    // The `+++`/`---` headers are file markers, not content.
    let (added, removed): (i64, i64) = store
        .query_row(
            "SELECT SUM(lines_added), SUM(lines_removed) FROM telemetry_edits",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(added, 5);
    assert_eq!(removed, 1);
}

#[test]
fn the_fixture_records_rate_limits_without_summing_them() {
    // Rate limits are an account-level gauge: four observations of one account,
    // stored as four readings, never accumulated into 86%.
    let store = ingest(&fixture_log());
    let (count, max): (i64, f64) = store
        .query_row(
            "SELECT count(*), MAX(used_percent) FROM telemetry_limits",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 4);
    assert_eq!(max, 31.75);
}

#[test]
fn summed_deltas_reproduce_the_providers_cumulative_gauge() {
    let Some(path) = newest_codex_log() else {
        eprintln!("skipped: no codex rollout log on this machine");
        return;
    };
    let Some(declared) = declared_totals(&path) else {
        eprintln!("skipped: {} carries no token_count records", path.display());
        return;
    };

    let store = ingest(&path);
    let (input, cached, output, reasoning) = ingested_totals(&store);

    // Guard against a vacuous pass: an empty log would satisfy equality while
    // proving nothing about the arithmetic.
    assert!(
        declared.0 > 0,
        "{} declares no input tokens, so this assertion would be vacuous",
        path.display()
    );
    // Cache reads must be a strict part of the prompt total on a real log, not
    // a separate series added beside it. This is the provider-semantics claim
    // the normalization rests on, checked against a codex Wardian does not
    // control rather than against a fixture this crate wrote.
    assert!(
        declared.1 < declared.0,
        "codex should report cached input as part of its prompt total in {}",
        path.display()
    );
    eprintln!(
        "real codex log {}: input={} cached={} output={} reasoning={}",
        path.display(),
        declared.0,
        declared.1,
        declared.2,
        declared.3
    );

    assert_eq!(
        (input + cached, cached, output, reasoning),
        declared,
        "summed last_token_usage must reconstruct the final total_token_usage in {}",
        path.display()
    );
}

#[test]
fn a_real_log_produces_usable_facts() {
    let Some(path) = newest_codex_log() else {
        eprintln!("skipped: no codex rollout log on this machine");
        return;
    };
    let store = ingest(&path);

    let intervals: i64 = store
        .query_row("SELECT count(*) FROM telemetry_activity", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(intervals > 0, "a real log should yield activity intervals");

    // Every stored interval must be a forward span.
    let inverted: i64 = store
        .query_row(
            "SELECT count(*) FROM telemetry_activity WHERE ended_at < started_at",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(inverted, 0);

    // Rollups must not disagree with the facts they summarize.
    let (fact_input, rollup_input): (i64, i64) = (
        store
            .query_row(
                "SELECT COALESCE(SUM(input_tokens),0) FROM telemetry_turns",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        store
            .query_row(
                "SELECT COALESCE(SUM(input_tokens),0) FROM telemetry_rollup_hourly",
                [],
                |row| row.get(0),
            )
            .unwrap(),
    );
    assert_eq!(fact_input, rollup_input);
}

#[test]
fn re_ingesting_a_real_log_is_idempotent() {
    let Some(path) = newest_codex_log() else {
        eprintln!("skipped: no codex rollout log on this machine");
        return;
    };

    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();
    let ctx = SourceContext::new("agent-real", "codex", &path);

    ingest_source(&store, &ctx).unwrap();
    let first = ingested_totals(&store);
    // Force a full re-read the way a parser version bump would.
    store
        .execute("UPDATE telemetry_sources SET parser_version = 0", [])
        .unwrap();
    ingest_source(&store, &ctx).unwrap();

    assert_eq!(ingested_totals(&store), first);
}

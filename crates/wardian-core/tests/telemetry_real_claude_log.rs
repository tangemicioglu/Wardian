//! The claude parser against a real Claude Code transcript.
//!
//! The fixture is 400 consecutive assistant records taken from an actual
//! session, with every piece of prose and every tool argument replaced by a
//! placeholder. What survives untouched is the accounting — the usage block,
//! the model, the record shape — because that is the part a parser can get
//! quietly and enormously wrong.
//!
//! The expected figures below were transcribed from the fixture by an
//! independent reading, not produced by this parser. A fixture whose
//! expectations come out of the code under test agrees with itself no matter
//! what either one does; that is exactly how a 49-fold token error survived a
//! green codex suite.

use wardian_core::telemetry::models::SourceCarry;
use wardian_core::telemetry::sources::claude::parse_delta;
use wardian_core::telemetry::sources::SourceContext;

/// Independently counted from `tests/fixtures/claude-transcript.jsonl`.
const EXPECTED_TURNS: usize = 400;
const EXPECTED_FRESH_INPUT: i64 = 8_446;
const EXPECTED_CACHE_READ: i64 = 80_194_623;
const EXPECTED_CACHE_WRITE: i64 = 1_885_871;
const EXPECTED_OUTPUT: i64 = 462_954;
const EXPECTED_EDITS: usize = 33;

fn parse_fixture() -> wardian_core::telemetry::models::ParsedFacts {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude-transcript.jsonl"),
    )
    .expect("fixture is readable");
    let ctx = SourceContext::new(
        "agent-1",
        "claude",
        std::path::Path::new("claude-transcript.jsonl"),
    );
    parse_delta(&ctx, &text, SourceCarry::default())
}

#[test]
fn every_assistant_reply_in_a_real_transcript_becomes_one_turn() {
    let facts = parse_fixture();
    assert_eq!(facts.turns.len(), EXPECTED_TURNS);
}

#[test]
fn real_token_totals_match_an_independent_count() {
    let facts = parse_fixture();
    let sum = |pick: fn(&wardian_core::telemetry::models::TurnFact) -> Option<i64>| {
        facts.turns.iter().filter_map(pick).sum::<i64>()
    };

    assert_eq!(sum(|turn| turn.input_tokens), EXPECTED_FRESH_INPUT);
    assert_eq!(sum(|turn| turn.cached_input_tokens), EXPECTED_CACHE_READ);
    assert_eq!(sum(|turn| turn.cache_write_tokens), EXPECTED_CACHE_WRITE);
    assert_eq!(sum(|turn| turn.output_tokens), EXPECTED_OUTPUT);
}

#[test]
fn fresh_input_is_not_contaminated_by_cache_reads() {
    // The claim this file exists to defend. On real traffic claude's cache reads
    // run four orders of magnitude above its fresh input, so treating the two as
    // interchangeable — in either direction — is not a rounding error.
    //
    // Storing `input + cache_read` as input would overstate usage roughly
    // 9,500-fold. Subtracting cache reads, which is the correction codex
    // genuinely needs, would clamp almost every turn to zero. Both mistakes
    // leave every individual figure looking plausible.
    let facts = parse_fixture();
    let fresh: i64 = facts.turns.iter().filter_map(|turn| turn.input_tokens).sum();
    let cached: i64 = facts
        .turns
        .iter()
        .filter_map(|turn| turn.cached_input_tokens)
        .sum();

    assert_eq!(fresh, EXPECTED_FRESH_INPUT);
    assert!(
        cached > fresh * 1_000,
        "fixture must exercise a cache-dominated session: fresh {fresh}, cached {cached}"
    );
    // The two are disjoint quantities, so neither may have absorbed the other.
    assert_ne!(fresh, cached + fresh);
}

#[test]
fn real_file_tool_calls_become_edits() {
    let facts = parse_fixture();
    assert_eq!(facts.edits.len(), EXPECTED_EDITS);
    assert!(facts
        .edits
        .iter()
        .all(|edit| edit.workspace.as_deref() == Some("D:/redacted")));
}

#[test]
fn every_record_is_offered_to_clustering() {
    // Claude reports no durations of its own, so active time is only ever as
    // good as the timestamps fed to the clusterer.
    let facts = parse_fixture();
    assert_eq!(facts.event_times.len(), EXPECTED_TURNS);
    assert!(facts.intervals.is_empty());
}

#[test]
fn re_reading_the_same_bytes_produces_the_same_keys() {
    // Ingest re-reads a source whenever the parser version moves or a cursor is
    // invalidated, and dedup is by event key. Unstable keys would turn every
    // repair into a double count.
    let first = parse_fixture();
    let second = parse_fixture();
    let keys = |facts: &wardian_core::telemetry::models::ParsedFacts| {
        facts
            .turns
            .iter()
            .map(|turn| turn.event_key.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&first), keys(&second));

    let unique: std::collections::HashSet<_> = keys(&first).into_iter().collect();
    assert_eq!(unique.len(), EXPECTED_TURNS, "keys must not collide");
}

//! The archive source against a real archived antigravity conversation.
//!
//! The fixture is every turn record from an actual conversation, with prose,
//! tool output and real paths replaced by placeholders. What survives untouched
//! is the structure the source depends on: `turn_key`, `started_at`,
//! `updated_at`, and the shape of `files.written`.
//!
//! This provider publishes no token accounting and no parseable transcript. The
//! claim under test is therefore not "the numbers are right" but something
//! narrower and more important: that a provider Wardian could not read natively
//! still yields real turns, real timestamps and real file edits — and that the
//! measures it genuinely lacks come back as unreported rather than as zero.

use chrono::{DateTime, Utc};
use wardian_core::telemetry::sources::archive::parse_turns;
use wardian_core::telemetry::sources::SourceContext;

/// Independently counted from `tests/fixtures/archive-turns.jsonl`.
const EXPECTED_TURNS: usize = 154;
const EXPECTED_EDITS: usize = 101;
const EXPECTED_EVENTS: usize = 308;

fn fixture() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/archive-turns.jsonl"),
    )
    .expect("fixture is readable")
}

fn ctx() -> SourceContext {
    SourceContext::new(
        "agent-1",
        "antigravity",
        std::path::Path::new("turns.jsonl"),
    )
}

/// Far enough past the newest record that every turn has settled.
fn well_after() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2027-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn a_provider_with_no_native_log_still_yields_its_turns() {
    let (facts, _) = parse_turns(&ctx(), &fixture(), 0, well_after());
    assert_eq!(facts.turns.len(), EXPECTED_TURNS);
    assert!(facts
        .turns
        .iter()
        .all(|turn| turn.provider == "antigravity"));
}

#[test]
fn the_measures_this_provider_lacks_are_unreported_not_zero() {
    // The distinction the whole nullable-token design exists to preserve. A zero
    // here would rank antigravity the cheapest provider in the habitat rather
    // than the unmeasured one.
    let (facts, _) = parse_turns(&ctx(), &fixture(), 0, well_after());
    assert!(facts.turns.iter().all(|turn| turn.input_tokens.is_none()));
    assert!(facts.turns.iter().all(|turn| turn.output_tokens.is_none()));
    assert!(facts
        .turns
        .iter()
        .all(|turn| turn.cached_input_tokens.is_none()));
    assert!(facts.edits.iter().all(|edit| edit.lines_added.is_none()));
}

#[test]
fn real_file_writes_become_edits() {
    let (facts, _) = parse_turns(&ctx(), &fixture(), 0, well_after());
    assert_eq!(facts.edits.len(), EXPECTED_EDITS);
    assert!(facts.edits.iter().all(|edit| !edit.path.is_empty()));
}

#[test]
fn every_turn_boundary_feeds_clustering() {
    // Active time for this provider is inferred entirely from these timestamps,
    // because nothing reports a duration.
    let (facts, _) = parse_turns(&ctx(), &fixture(), 0, well_after());
    assert_eq!(facts.event_times.len(), EXPECTED_EVENTS);
    assert!(facts.intervals.is_empty());
}

#[test]
fn identity_survives_the_file_being_rewritten_whole() {
    // `turns.jsonl` is rewritten atomically rather than appended, so the same
    // records are re-read constantly. Identity comes from the archive's own
    // `turn_key`, which does not move when the file does.
    let text = fixture();
    let (first, _) = parse_turns(&ctx(), &text, 0, well_after());
    let (second, _) = parse_turns(&ctx(), &text, 0, well_after());

    let keys = |facts: &wardian_core::telemetry::models::ParsedFacts| {
        facts
            .turns
            .iter()
            .map(|turn| turn.event_key.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&first), keys(&second));

    let unique: std::collections::HashSet<_> = keys(&first).into_iter().collect();
    assert_eq!(unique.len(), EXPECTED_TURNS, "turn keys must not collide");
}

#[test]
fn a_second_pass_over_an_unchanged_conversation_reads_nothing() {
    // The steady state. Without this every pass would re-offer every turn in
    // every archived conversation the habitat has ever had.
    let text = fixture();
    let (_, cursor) = parse_turns(&ctx(), &text, 0, well_after());
    let (again, next) = parse_turns(&ctx(), &text, cursor.value, well_after());

    assert!(again.turns.is_empty());
    assert!(again.edits.is_empty());
    assert_eq!(next.value, cursor.value);
}

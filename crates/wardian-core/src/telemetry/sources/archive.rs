//! Telemetry from Wardian's own conversation archive.
//!
//! Some providers publish no machine-readable log of their own. Antigravity is
//! the standing example: it reports no token accounting at all (corroborated by
//! ccusage, which lists it unsupported for that reason), and its editor state is
//! not a transcript anything can parse. Treating that as "no telemetry" was
//! wrong, because Wardian *watched the whole thing happen* — on the habitat this
//! was built against, the archive already held 784 turns across 31 antigravity
//! conversations, each with timestamps, tool calls and the files it wrote.
//!
//! So this source reads `turns.jsonl` from the archive rather than a provider
//! log. What it can offer is genuinely narrower than a native source — there are
//! no tokens and no line counts, and it says so by leaving those `None` rather
//! than filling in zeros. Everything it does offer is first-hand.
//!
//! It is deliberately a *fallback*: [`super::source_for`] routes a provider here
//! only when that provider has no native reader, so a codex agent is never read
//! twice through two different lenses.
//!
//! ## Why the cursor is a timestamp
//!
//! `turns.jsonl` is rewritten atomically each time a turn advances, not
//! appended. A byte offset into it means nothing after the next rewrite, so this
//! advances on `updated_at` the way the opencode source does, and takes identity
//! from the archive's own `turn_key`.

use crate::telemetry::models::{
    Cursor, CursorKind, EditFact, EditOp, ParsedFacts, SourceCarry, SourceKind, TurnFact,
};
use crate::telemetry::sources::{SourceContext, SourceError, TelemetrySource};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// How long a turn must be quiet before it is ingested.
///
/// Facts are written with `INSERT OR IGNORE`, so the first version of a turn is
/// the one that sticks. A turn read while it is still running would therefore be
/// frozen with whatever it had done so far, and the files it wrote afterwards
/// would never appear. Waiting for it to settle costs a little latency on the
/// newest turn and buys correctness on every other one.
const SETTLE_MS: i64 = 120_000;

/// The archive's status for a turn that has not been answered yet.
///
/// The only status that is *known* to be non-terminal. Most archived turns are
/// recorded as `unknown`, so requiring a terminal status would discard the
/// majority of real history; excluding the one status that positively says
/// "still running" is the honest half of that trade. A turn that is `unknown`
/// and still growing after the settle window can still be frozen early — the
/// residual this cannot close without a completion signal the archive does not
/// supply.
const PENDING_STATUS: &str = "pending_response";

pub struct ArchiveSource;

impl TelemetrySource for ArchiveSource {
    /// The medium, not the provider. A record's provider comes from the context,
    /// because this one reader serves every provider that lacks a native log.
    fn provider(&self) -> &'static str {
        "archive"
    }

    fn parser_version(&self) -> i64 {
        1
    }

    fn source_kind(&self) -> SourceKind {
        SourceKind::Jsonl
    }

    fn cursor_kind(&self) -> CursorKind {
        CursorKind::EpochMs
    }

    fn read_since(
        &self,
        ctx: &SourceContext,
        cursor: Cursor,
        _carry: SourceCarry,
    ) -> Result<(ParsedFacts, Cursor), SourceError> {
        let text = std::fs::read_to_string(&ctx.path)
            .map_err(|err| SourceError::Unavailable(err.to_string()))?;
        Ok(parse_turns(ctx, &text, cursor.value, Utc::now()))
    }
}

/// Pure parse of an archive turn file. No filesystem, no clock of its own.
pub fn parse_turns(
    ctx: &SourceContext,
    text: &str,
    since_ms: i64,
    now: DateTime<Utc>,
) -> (ParsedFacts, Cursor) {
    let settled_before = now.timestamp_millis() - SETTLE_MS;
    let mut facts = ParsedFacts::default();
    let mut next = since_ms;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        let Some(turn_key) = string_field(&record, "turn_key") else {
            continue;
        };
        let Some(updated_at) = string_field(&record, "updated_at") else {
            continue;
        };
        let Some(updated_ms) = epoch_ms(&updated_at) else {
            continue;
        };

        // Already consumed, or not yet finished changing.
        if updated_ms <= since_ms || updated_ms > settled_before {
            continue;
        }

        // A turn still awaiting its reply is not finished, however long it has
        // been quiet. Facts are written `INSERT OR IGNORE`, so ingesting one now
        // would freeze it at the work it had done so far and silently ignore
        // everything it did afterwards — the files it went on to write would
        // never appear. Waiting costs latency on one turn and nothing else.
        if string_field(&record, "status").as_deref() == Some(PENDING_STATUS) {
            continue;
        }
        next = next.max(updated_ms);

        let started_at = string_field(&record, "started_at");

        // Both ends of the turn feed clustering. A turn is request-to-response
        // and includes however long the person took to reply, so treating its
        // span as measured working time would credit thinking time as work; the
        // clusterer splits it on its own gap rule instead.
        if let Some(started) = &started_at {
            facts.event_times.push(started.clone());
        }
        facts.event_times.push(updated_at.clone());

        facts.turns.push(TurnFact {
            event_key: turn_key.clone(),
            session_id: ctx.session_id.clone(),
            provider: ctx.provider.clone(),
            turn_id: Some(turn_key.clone()),
            // The archive records which provider answered, never which model.
            model: None,
            effort: None,
            started_at,
            ended_at: updated_at.clone(),
            // Not zero — unknown. A provider that publishes no token accounting
            // has not used zero tokens, and every surface renders these as
            // unreported precisely so it cannot be ranked as the cheapest.
            input_tokens: None,
            cached_input_tokens: None,
            cache_write_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            context_window: None,
            cost_usd: None,
        });

        for (index, path) in written_paths(&record).into_iter().enumerate() {
            facts.edits.push(EditFact {
                event_key: format!("{turn_key}:{index}"),
                session_id: ctx.session_id.clone(),
                provider: ctx.provider.clone(),
                turn_id: Some(turn_key.clone()),
                occurred_at: updated_at.clone(),
                workspace: None,
                path,
                // The archive records that a file was written, never a diff, so
                // the operation is the most that can be claimed.
                op: EditOp::Update,
                // Unknown rather than zero, for the same reason as the tokens.
                lines_added: None,
                lines_removed: None,
            });
        }
    }

    (facts, Cursor::new(CursorKind::EpochMs, next))
}

/// Distinct files a turn wrote.
///
/// `files.written` is the archive's own record of the turn's effects. It is
/// deduplicated because the same path is recorded in more than one form when a
/// tool reports it with different path separators.
fn written_paths(record: &Value) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let Some(written) = record
        .get("files")
        .and_then(|files| files.get("written"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    for entry in written {
        if let Some(path) = entry.as_str().filter(|path| !path.is_empty()) {
            seen.insert(path.replace('\\', "/"));
        }
    }
    seen.into_iter().collect()
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|found| !found.is_empty())
}

fn epoch_ms(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc).timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> SourceContext {
        SourceContext::new(
            "agent-1",
            "antigravity",
            std::path::Path::new("turns.jsonl"),
        )
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-22T15:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Shaped after a real archived antigravity turn. One line, because the
    /// archive writes one JSON record per line and the parser reads it that way.
    const TURN: &str = r#"{"schema":3,"turn_key":"conv_1:turn:000002","turn_index":2,"status":"responded","started_at":"2026-07-22T14:32:56.128Z","updated_at":"2026-07-22T14:33:20.174Z","counts":{"tool_calls":9},"tools_used":{"Write file":2,"View file":3},"files":{"read":["a.md"],"written":["C:\\work\\a.rs","C:/work/b.rs"],"mentioned":[]}}"#;

    fn parse(text: &str) -> ParsedFacts {
        parse_turns(&ctx(), text, 0, now()).0
    }

    #[test]
    fn an_archived_turn_becomes_a_turn_fact() {
        let facts = parse(TURN);
        assert_eq!(facts.turns.len(), 1);
        let turn = &facts.turns[0];
        assert_eq!(turn.provider, "antigravity");
        assert_eq!(turn.event_key, "conv_1:turn:000002");
        assert_eq!(turn.ended_at, "2026-07-22T14:33:20.174Z");
        assert_eq!(turn.started_at.as_deref(), Some("2026-07-22T14:32:56.128Z"));
    }

    #[test]
    fn tokens_are_unreported_rather_than_zero() {
        // The whole point of admitting this provider. Antigravity publishes no
        // token accounting, and a zero would rank it the cheapest agent in the
        // habitat rather than the unmeasured one.
        let turn = &parse(TURN).turns[0];
        assert_eq!(turn.input_tokens, None);
        assert_eq!(turn.cached_input_tokens, None);
        assert_eq!(turn.output_tokens, None);
        assert_eq!(turn.reasoning_tokens, None);
    }

    #[test]
    fn written_files_become_edits_with_unknown_line_counts() {
        let facts = parse(TURN);
        assert_eq!(facts.edits.len(), 2);
        assert_eq!(facts.edits[0].path, "C:/work/a.rs");
        assert_eq!(facts.edits[0].lines_added, None);
        assert_eq!(facts.edits[0].lines_removed, None);
    }

    #[test]
    fn a_path_recorded_in_two_separator_styles_is_one_file() {
        // The archive stores whatever the tool reported, and antigravity reports
        // the same file both ways. Counting both would inflate files touched.
        let line = r#"{"turn_key":"t1","updated_at":"2026-07-22T14:33:20.174Z","files":{"written":["C:\\work\\a.rs","C:/work/a.rs"]}}"#;
        let facts = parse(line);
        assert_eq!(facts.edits.len(), 1);
    }

    #[test]
    fn both_ends_of_a_turn_feed_clustering() {
        // Active time for these providers is inferred, so it is only ever as
        // good as the timestamps offered to the clusterer.
        let facts = parse(TURN);
        assert_eq!(
            facts.event_times,
            vec!["2026-07-22T14:32:56.128Z", "2026-07-22T14:33:20.174Z"]
        );
        assert!(facts.intervals.is_empty());
    }

    #[test]
    fn a_turn_awaiting_its_reply_is_not_ingested_however_quiet_it_is() {
        // Long quiet is not completion. Facts are written INSERT OR IGNORE, so
        // storing an unanswered turn freezes it at the work it had done and
        // discards everything it does afterwards.
        let line = r#"{"turn_key":"t1","status":"pending_response","updated_at":"2026-07-22T14:33:20.174Z","files":{"written":["a.rs"]}}"#;
        let facts = parse(line);
        assert!(facts.turns.is_empty());
        assert!(facts.edits.is_empty());
    }

    #[test]
    fn a_turn_with_no_recorded_status_still_settles() {
        // Most archived turns carry `unknown`. Requiring a terminal status would
        // throw away the majority of real history.
        let line = r#"{"turn_key":"t1","status":"unknown","updated_at":"2026-07-22T14:33:20.174Z","files":{"written":["a.rs"]}}"#;
        assert_eq!(parse(line).turns.len(), 1);
    }

    #[test]
    fn a_turn_still_being_written_is_left_for_a_later_pass() {
        // Facts are written with INSERT OR IGNORE, so a turn read mid-flight
        // would be frozen with the work it had done so far and never corrected.
        let line = r#"{"turn_key":"t1","updated_at":"2026-07-22T14:59:30.000Z","files":{"written":["a.rs"]}}"#;
        let (facts, cursor) = parse_turns(&ctx(), line, 0, now());
        assert!(facts.turns.is_empty());
        assert_eq!(
            cursor.value, 0,
            "an unsettled turn must not move the cursor"
        );
    }

    #[test]
    fn turns_already_consumed_are_not_read_again() {
        let (facts, _) = parse_turns(
            &ctx(),
            TURN,
            epoch_ms("2026-07-22T14:33:20.174Z").unwrap(),
            now(),
        );
        assert!(facts.turns.is_empty());
    }

    #[test]
    fn the_cursor_advances_to_the_newest_settled_turn() {
        let (_, cursor) = parse_turns(&ctx(), TURN, 0, now());
        assert_eq!(cursor.value, epoch_ms("2026-07-22T14:33:20.174Z").unwrap());
    }

    #[test]
    fn a_rewritten_file_re_reads_without_duplicating() {
        // turns.jsonl is rewritten whole, not appended, so the same records are
        // seen repeatedly. Identity comes from the archive's own turn key, which
        // does not move when the file does.
        let first = parse(TURN);
        let second = parse(&format!("{TURN}\n{TURN}"));
        assert_eq!(first.turns[0].event_key, second.turns[0].event_key);
        assert_eq!(second.turns[0].event_key, second.turns[1].event_key);
    }

    #[test]
    fn malformed_and_incomplete_records_are_skipped_not_fatal() {
        let facts = parse(&format!("not json\n{{\"turn_key\":\"t\"}}\n{TURN}\n"));
        assert_eq!(facts.turns.len(), 1);
    }
}

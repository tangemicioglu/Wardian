//! Claude Code transcript source.
//!
//! Claude Code writes one append-only JSONL transcript per session, named after
//! the session id, under `~/.claude/projects/<encoded-cwd>/`. Structurally it is
//! the same medium as a codex rollout — advanced by byte offset, one JSON record
//! per line — so it reuses [`read_delta`] rather than reimplementing the
//! partial-line and truncation handling.
//!
//! What differs is the accounting. Claude reports prompt tokens the way the
//! Anthropic API does: `input_tokens` counts only what was *not* served from
//! cache, with cache reads and cache writes reported alongside it as disjoint
//! quantities. That is already the normalization [`TurnFact::input_tokens`]
//! requires, so unlike codex this source subtracts nothing. Getting that
//! backwards in either direction is the 49-fold error the codex parser was fixed
//! for, so the disjointness is asserted by test against a real transcript rather
//! than assumed.

use crate::telemetry::identity::{content_key, file_fingerprint};
use crate::telemetry::models::{
    Cursor, CursorKind, EditFact, EditOp, ParsedFacts, SourceCarry, SourceKind, TurnFact,
};
use crate::telemetry::sources::codex::read_delta;
use crate::telemetry::sources::{SourceContext, SourceError, TelemetrySource};
use serde_json::Value;

/// Model name Claude Code stamps on replies it generated itself.
const SYNTHETIC_MODEL: &str = "<synthetic>";

pub struct ClaudeSource;

impl TelemetrySource for ClaudeSource {
    fn provider(&self) -> &'static str {
        "claude"
    }

    fn parser_version(&self) -> i64 {
        1
    }

    fn source_kind(&self) -> SourceKind {
        SourceKind::Jsonl
    }

    fn cursor_kind(&self) -> CursorKind {
        CursorKind::ByteOffset
    }

    fn read_since(
        &self,
        ctx: &SourceContext,
        cursor: Cursor,
        carry: SourceCarry,
    ) -> Result<(ParsedFacts, Cursor), SourceError> {
        let (delta, next_offset) = read_delta(&ctx.path, cursor.value)?;
        let base_offset = next_offset - delta.len() as i64;
        let facts = parse_delta_at(ctx, &delta, base_offset, carry);
        Ok((facts, Cursor::new(CursorKind::ByteOffset, next_offset)))
    }

    /// A byte offset only means something against the bytes it was taken from.
    fn cursor_is_stale(&self, ctx: &SourceContext, stored_fingerprint: Option<&str>) -> bool {
        match (stored_fingerprint, file_fingerprint(&ctx.path)) {
            (Some(stored), Some(current)) => stored != current,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }
}

/// Pure parse of a complete-line delta. No filesystem, no database.
pub fn parse_delta(ctx: &SourceContext, delta: &str, carry: SourceCarry) -> ParsedFacts {
    parse_delta_at(ctx, delta, 0, carry)
}

/// As [`parse_delta`], but told where in the file the delta begins.
///
/// The offset makes each record's key positional as well as content-derived, for
/// the same reason it does in the codex parser: two records cannot share an
/// offset, so the pair is unique by construction.
pub fn parse_delta_at(
    ctx: &SourceContext,
    delta: &str,
    base_offset: i64,
    carry: SourceCarry,
) -> ParsedFacts {
    let mut facts = ParsedFacts::default();
    // Claude records their working directory on every entry, so unlike codex
    // there is no separate context record to inherit from. The carry is kept so
    // a delta that opens on records without one still attributes them.
    let mut current_cwd = carry.cwd;
    let mut current_model = carry.model;

    let mut line_offset = base_offset;
    for line in delta.split_inclusive('\n') {
        let record_offset = line_offset;
        line_offset += line.len() as i64;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        let Some(timestamp) = record.get("timestamp").and_then(Value::as_str) else {
            // Transcripts carry unstamped bookkeeping records (mode changes,
            // titles). They are not events and must not feed clustering.
            continue;
        };
        facts.event_times.push(timestamp.to_string());

        if let Some(cwd) = string_field(&record, "cwd") {
            current_cwd = Some(cwd);
        }

        let message = record.get("message");
        // A synthetic reply must not become the carried model either, or the
        // next record that omits one would inherit a model that never ran.
        if let Some(model) = message
            .and_then(|message| string_field(message, "model"))
            .filter(|model| model != SYNTHETIC_MODEL)
        {
            current_model = Some(model);
        }

        let event_key = format!("{record_offset:012x}:{}", content_key(trimmed.as_bytes()));

        if record.get("type").and_then(Value::as_str) == Some("assistant") {
            if let Some(message) = message {
                if let Some(turn) =
                    parse_turn(ctx, message, timestamp, &event_key, current_model.clone())
                {
                    facts.turns.push(turn);
                }
                collect_edits(
                    ctx,
                    message,
                    timestamp,
                    &event_key,
                    current_cwd.as_deref(),
                    &mut facts.edits,
                );
            }
        }
    }

    facts.carry = SourceCarry {
        turn_id: None,
        model: current_model,
        effort: None,
        cwd: current_cwd,
    };
    facts
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|found| !found.is_empty())
}

/// One assistant reply's token accounting.
fn parse_turn(
    ctx: &SourceContext,
    message: &Value,
    timestamp: &str,
    event_key: &str,
    model: Option<String>,
) -> Option<TurnFact> {
    let usage = message.get("usage")?;

    // Claude Code writes locally generated replies — cancellations, "no response
    // requested" — as assistant records carrying an all-zero usage block under a
    // `<synthetic>` model. They are not model calls, so counting them would add
    // turns nobody took and attribute them to a model that does not exist.
    if message.get("model").and_then(Value::as_str) == Some(SYNTHETIC_MODEL) {
        return None;
    }

    Some(TurnFact {
        event_key: event_key.to_string(),
        session_id: ctx.session_id.clone(),
        provider: "claude".to_string(),
        turn_id: string_field(message, "id"),
        model,
        effort: None,
        started_at: None,
        ended_at: timestamp.to_string(),
        // Taken raw, deliberately. Claude reports prompt tokens disjointly:
        // `input_tokens` already excludes everything served from cache, which is
        // exactly what this column means. Subtracting cache reads here — the
        // adjustment codex needs — would drive most turns to zero.
        input_tokens: usage.get("input_tokens").and_then(Value::as_i64),
        cached_input_tokens: usage.get("cache_read_input_tokens").and_then(Value::as_i64),
        cache_write_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_i64),
        // Claude bills thinking as output rather than reporting it apart, so
        // there is no figure here to record. A zero would read as "did no
        // reasoning" rather than "does not say".
        reasoning_tokens: None,
        context_window: None,
        cost_usd: None,
    })
}

/// Tool calls in one reply that changed a file.
///
/// Only the provider's own file tools are counted. An agent writing through a
/// shell redirect leaves no structured record, which is the same lower-bound
/// caveat every other source carries.
fn collect_edits(
    ctx: &SourceContext,
    message: &Value,
    timestamp: &str,
    event_key: &str,
    cwd: Option<&str>,
    edits: &mut Vec<EditFact>,
) {
    let Some(blocks) = message.get("content").and_then(Value::as_array) else {
        return;
    };

    for (index, block) in blocks.iter().enumerate() {
        if block.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let name = block.get("name").and_then(Value::as_str).unwrap_or("");
        let Some(input) = block.get("input") else {
            continue;
        };
        let Some(path) = string_field(input, "file_path") else {
            continue;
        };

        let (op, added, removed) = match name {
            "Write" => {
                let lines = input
                    .get("content")
                    .and_then(Value::as_str)
                    .map(count_lines)
                    .unwrap_or(0);
                (EditOp::Add, lines, 0)
            }
            "Edit" => {
                let (added, removed) = replacement_lines(input);
                (EditOp::Update, added, removed)
            }
            "MultiEdit" | "NotebookEdit" => {
                // One record covers several replacements in the same file, so
                // the counts are the sum rather than the first.
                let (mut added, mut removed) = (0, 0);
                let entries = input
                    .get("edits")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for entry in &entries {
                    let (entry_added, entry_removed) = replacement_lines(entry);
                    added += entry_added;
                    removed += entry_removed;
                }
                if entries.is_empty() {
                    let (entry_added, entry_removed) = replacement_lines(input);
                    added += entry_added;
                    removed += entry_removed;
                }
                (EditOp::Update, added, removed)
            }
            _ => continue,
        };

        edits.push(EditFact {
            // Several tool calls can share one record, so the block's position
            // has to enter the key or they would collide into one edit.
            event_key: format!("{event_key}:{index}"),
            session_id: ctx.session_id.clone(),
            provider: "claude".to_string(),
            turn_id: string_field(message, "id"),
            occurred_at: timestamp.to_string(),
            workspace: cwd.map(str::to_string),
            path,
            op,
            lines_added: Some(added),
            lines_removed: Some(removed),
        });
    }
}

/// Lines added and removed by one string replacement.
fn replacement_lines(input: &Value) -> (i64, i64) {
    let old = input
        .get("old_string")
        .and_then(Value::as_str)
        .map(count_lines)
        .unwrap_or(0);
    let new = input
        .get("new_string")
        .and_then(Value::as_str)
        .map(count_lines)
        .unwrap_or(0);
    (new, old)
}

/// Lines in a block of text, counting a final line without a trailing newline.
fn count_lines(content: &str) -> i64 {
    if content.is_empty() {
        return 0;
    }
    content.lines().count() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> SourceContext {
        SourceContext::new("agent-1", "claude", std::path::Path::new("transcript.jsonl"))
    }

    fn parse(delta: &str) -> ParsedFacts {
        parse_delta(&ctx(), delta, SourceCarry::default())
    }

    const ASSISTANT: &str = r#"{"type":"assistant","timestamp":"2026-08-04T21:58:25.256Z","cwd":"D:\\work","sessionId":"s1","message":{"id":"msg_1","model":"claude-sonnet-5","usage":{"input_tokens":2,"cache_creation_input_tokens":17493,"cache_read_input_tokens":26514,"output_tokens":448}}}"#;

    #[test]
    fn an_assistant_reply_becomes_a_turn() {
        let facts = parse(ASSISTANT);
        assert_eq!(facts.turns.len(), 1);
        let turn = &facts.turns[0];
        assert_eq!(turn.provider, "claude");
        assert_eq!(turn.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(turn.turn_id.as_deref(), Some("msg_1"));
        assert_eq!(turn.ended_at, "2026-08-04T21:58:25.256Z");
    }

    #[test]
    fn cached_prompt_tokens_stay_out_of_fresh_input() {
        // The load-bearing difference from codex. Claude's `input_tokens` is
        // already cache-exclusive, so subtracting cache reads the way the codex
        // parser must would drive nearly every turn to zero — the same class of
        // error as the 49-fold overstatement, in the other direction.
        let turn = &parse(ASSISTANT).turns[0];
        assert_eq!(turn.input_tokens, Some(2));
        assert_eq!(turn.cached_input_tokens, Some(26_514));
        assert_eq!(turn.cache_write_tokens, Some(17_493));
        assert_eq!(turn.output_tokens, Some(448));
    }

    #[test]
    fn reasoning_is_absent_rather_than_zero() {
        // Claude bills thinking as output. A zero would claim it did none.
        assert_eq!(parse(ASSISTANT).turns[0].reasoning_tokens, None);
    }

    #[test]
    fn a_write_counts_the_lines_it_wrote() {
        let line = r#"{"type":"assistant","timestamp":"2026-08-04T21:58:25.256Z","cwd":"D:\\work","message":{"id":"m1","content":[{"type":"tool_use","name":"Write","input":{"file_path":"a.rs","content":"one\ntwo\nthree"}}]}}"#;
        let facts = parse(line);
        assert_eq!(facts.edits.len(), 1);
        assert_eq!(facts.edits[0].path, "a.rs");
        assert_eq!(facts.edits[0].op, EditOp::Add);
        assert_eq!(facts.edits[0].lines_added, Some(3));
        assert_eq!(facts.edits[0].workspace.as_deref(), Some("D:\\work"));
    }

    #[test]
    fn an_edit_counts_both_sides_of_the_replacement() {
        let line = r#"{"type":"assistant","timestamp":"2026-08-04T21:58:25.256Z","message":{"id":"m1","content":[{"type":"tool_use","name":"Edit","input":{"file_path":"a.rs","old_string":"one\ntwo","new_string":"1\n2\n3"}}]}}"#;
        let edit = &parse(line).edits[0];
        assert_eq!(edit.op, EditOp::Update);
        assert_eq!(edit.lines_added, Some(3));
        assert_eq!(edit.lines_removed, Some(2));
    }

    #[test]
    fn several_edits_in_one_reply_do_not_collide() {
        // They share a record and therefore a byte offset; without the block
        // index in the key the uniqueness constraint would keep only one.
        let line = r#"{"type":"assistant","timestamp":"2026-08-04T21:58:25.256Z","message":{"id":"m1","content":[{"type":"tool_use","name":"Write","input":{"file_path":"a.rs","content":"x"}},{"type":"tool_use","name":"Write","input":{"file_path":"b.rs","content":"y"}}]}}"#;
        let facts = parse(line);
        assert_eq!(facts.edits.len(), 2);
        assert_ne!(facts.edits[0].event_key, facts.edits[1].event_key);
    }

    #[test]
    fn a_multi_edit_sums_its_replacements() {
        let line = r#"{"type":"assistant","timestamp":"2026-08-04T21:58:25.256Z","message":{"id":"m1","content":[{"type":"tool_use","name":"MultiEdit","input":{"file_path":"a.rs","edits":[{"old_string":"a","new_string":"1\n2"},{"old_string":"b\nc","new_string":"3"}]}}]}}"#;
        let edit = &parse(line).edits[0];
        assert_eq!(edit.lines_added, Some(3));
        assert_eq!(edit.lines_removed, Some(3));
    }

    #[test]
    fn a_non_file_tool_is_not_an_edit() {
        let line = r#"{"type":"assistant","timestamp":"2026-08-04T21:58:25.256Z","message":{"id":"m1","content":[{"type":"tool_use","name":"WebSearch","input":{"query":"x"}}]}}"#;
        assert!(parse(line).edits.is_empty());
    }

    #[test]
    fn unstamped_bookkeeping_records_are_not_events() {
        // Transcripts interleave mode changes and titles with real activity.
        // Feeding them to the clusterer would invent working time.
        let line = r#"{"type":"mode","mode":"default"}"#;
        let facts = parse(line);
        assert!(facts.event_times.is_empty());
        assert!(facts.turns.is_empty());
    }

    #[test]
    fn every_stamped_record_feeds_clustering() {
        // Claude reports no durations, so activity has to be inferred from when
        // records appear.
        let facts = parse(ASSISTANT);
        assert_eq!(facts.event_times, vec!["2026-08-04T21:58:25.256Z"]);
        assert!(facts.intervals.is_empty());
    }

    #[test]
    fn malformed_lines_are_skipped_not_fatal() {
        let facts = parse(&format!("{{not json\n{ASSISTANT}\n"));
        assert_eq!(facts.turns.len(), 1);
    }

    #[test]
    fn a_reply_without_usage_is_not_a_turn() {
        let line = r#"{"type":"assistant","timestamp":"2026-08-04T21:58:25.256Z","message":{"id":"m1","content":[]}}"#;
        assert!(parse(line).turns.is_empty());
    }

    #[test]
    fn the_model_carries_across_records_that_omit_it() {
        let bare = r#"{"type":"assistant","timestamp":"2026-08-04T22:00:00.000Z","message":{"id":"m2","usage":{"input_tokens":5}}}"#;
        let facts = parse(&format!("{ASSISTANT}\n{bare}\n"));
        assert_eq!(facts.turns[1].model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(facts.carry.model.as_deref(), Some("claude-sonnet-5"));
    }

    #[test]
    fn event_keys_are_stable_across_where_a_delta_was_cut() {
        // The whole file read at once and the same file read in two pieces must
        // produce identical keys, or a re-read double counts.
        let whole = format!("{ASSISTANT}\n{ASSISTANT}\n");
        let full = parse_delta_at(&ctx(), &whole, 0, SourceCarry::default());

        let cut = ASSISTANT.len() + 1;
        let first = parse_delta_at(&ctx(), &whole[..cut], 0, SourceCarry::default());
        let second = parse_delta_at(&ctx(), &whole[cut..], cut as i64, first.carry.clone());

        let split: Vec<_> = first
            .turns
            .iter()
            .chain(second.turns.iter())
            .map(|turn| turn.event_key.clone())
            .collect();
        let joined: Vec<_> = full
            .turns
            .iter()
            .map(|turn| turn.event_key.clone())
            .collect();
        assert_eq!(split, joined);
    }
}

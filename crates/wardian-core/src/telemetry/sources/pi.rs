//! Pi coding agent session source.
//!
//! Pi writes one append-only JSONL session log per conversation under
//! `~/.wardian/agents/<agent>/pi/sessions/`, opening with a
//! `{"type":"session",...}` header that carries the session id and `cwd`.
//! Structurally that is the same medium as a codex rollout or a claude
//! transcript — advanced by byte offset, one JSON record per line — so it reuses
//! [`read_delta`] rather than reimplementing partial-line and truncation
//! handling.
//!
//! # Accounting
//!
//! Pi reports the richest usage of any provider Wardian reads. Its `Usage` type
//! (`@earendil-works/pi-ai`) is disjoint by construction:
//!
//! ```text
//! { input, output, cacheRead, cacheWrite, cacheWrite1h?, reasoning?,
//!   totalTokens, cost: { input, output, cacheRead, cacheWrite, total } }
//! ```
//!
//! Three properties decide how this source behaves, and each is asserted by
//! test rather than trusted:
//!
//! 1. **`input` is cache-exclusive**, so it is stored raw. `totalTokens` equals
//!    `input + output + cacheRead` on observed records, which is what proves
//!    cache reads sit *outside* `input`. Applying codex's subtraction here would
//!    drive most turns to zero — the same 49-fold error in the other direction.
//! 2. **`reasoning` is a subset of `output`**, documented as such upstream:
//!    "`output` already includes these tokens". It is recorded for reporting and
//!    never added to a total.
//! 3. **`usage` is per-message.** Pi's `--mode json` *stream* carries a
//!    top-level `usage` that is explicitly cumulative, but that is a different
//!    field on a different record type. The session file persists an
//!    `AssistantMessage`, whose `usage` covers that message alone. Reading the
//!    cumulative one as if it were a delta would multiply a session's tokens by
//!    roughly its message count, and every figure would stay plausible while
//!    doing it.
//!
//! Pi also reports real per-component `cost`, and a `cacheWrite1h` split that
//! Anthropic prices differently from 5-minute writes. Neither has a column yet;
//! `cost.total` is captured into `cost_usd` alongside opencode's, and the 1h
//! split is deliberately left on the floor until something needs it.

use crate::telemetry::identity::{content_key, file_fingerprint};
use crate::telemetry::models::{
    Cursor, CursorKind, EditFact, EditOp, ParsedFacts, SourceCarry, SourceKind, TurnFact,
};
use crate::telemetry::sources::codex::read_delta;
use crate::telemetry::sources::{SourceContext, SourceError, TelemetrySource};
use serde_json::Value;

pub struct PiSource;

impl TelemetrySource for PiSource {
    fn provider(&self) -> &'static str {
        "pi"
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
/// The offset makes each record's key positional as well as content-derived,
/// for the same reason it does in the codex and claude parsers: two records
/// cannot share an offset, so the pair is unique by construction.
///
/// `carry` matters more here than it looks. Pi records `cwd` exactly once, in
/// the session header, and the thinking level in its own record — both
/// routinely land in an earlier delta than the messages that inherit them. A
/// parse that started empty would attribute the same record differently
/// depending on when ingest happened to run.
pub fn parse_delta_at(
    ctx: &SourceContext,
    delta: &str,
    base_offset: i64,
    carry: SourceCarry,
) -> ParsedFacts {
    let mut facts = ParsedFacts::default();
    let mut current_cwd = carry.cwd;
    let mut current_model = carry.model;
    let mut current_effort = carry.effort;

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

        let kind = record.get("type").and_then(Value::as_str).unwrap_or("");

        // Read before the timestamp guard: these carry one, but the context
        // they establish must survive even if a future version drops it.
        match kind {
            "session" => {
                if let Some(cwd) = string_field(&record, "cwd") {
                    current_cwd = Some(cwd);
                }
            }
            "model_change" => {
                if let Some(model) = string_field(&record, "modelId") {
                    current_model = Some(model);
                }
            }
            "thinking_level_change" => {
                if let Some(level) = string_field(&record, "thinkingLevel") {
                    current_effort = Some(level);
                }
            }
            _ => {}
        }

        let Some(timestamp) = record.get("timestamp").and_then(Value::as_str) else {
            // Bookkeeping records without a stamp are not events and must not
            // feed clustering.
            continue;
        };
        facts.event_times.push(timestamp.to_string());

        if kind != "message" {
            continue;
        }
        let Some(message) = record.get("message") else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }

        // The message names its own model, which is more precise than the
        // carried one: a `model_change` mid-session applies from the next
        // request, not retroactively.
        if let Some(model) = string_field(message, "model") {
            current_model = Some(model);
        }

        let event_key = format!("{record_offset:012x}:{}", content_key(trimmed.as_bytes()));
        let turn_id = string_field(message, "responseId").or_else(|| string_field(&record, "id"));

        if let Some(turn) = parse_turn(
            ctx,
            message,
            timestamp,
            &event_key,
            turn_id.clone(),
            current_model.clone(),
            current_effort.clone(),
        ) {
            facts.turns.push(turn);
        }
        collect_edits(
            ctx,
            message,
            timestamp,
            &event_key,
            turn_id,
            current_cwd.as_deref(),
            &mut facts.edits,
        );
    }

    facts.carry = SourceCarry {
        turn_id: None,
        model: current_model,
        effort: current_effort,
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
#[allow(clippy::too_many_arguments)]
fn parse_turn(
    ctx: &SourceContext,
    message: &Value,
    timestamp: &str,
    event_key: &str,
    turn_id: Option<String>,
    model: Option<String>,
    effort: Option<String>,
) -> Option<TurnFact> {
    let usage = message.get("usage")?;

    Some(TurnFact {
        event_key: event_key.to_string(),
        session_id: ctx.session_id.clone(),
        provider: "pi".to_string(),
        turn_id,
        model,
        effort,
        started_at: None,
        ended_at: timestamp.to_string(),
        // Stored raw. Pi reports prompt tokens disjointly — `totalTokens`
        // reconciles as `input + output + cacheRead`, so cache reads are not
        // inside `input` and there is nothing to subtract.
        input_tokens: usage.get("input").and_then(Value::as_i64),
        cached_input_tokens: usage.get("cacheRead").and_then(Value::as_i64),
        cache_write_tokens: usage.get("cacheWrite").and_then(Value::as_i64),
        output_tokens: usage.get("output").and_then(Value::as_i64),
        // A subset of `output`, recorded but never summed into a total. Absent
        // for providers that publish no breakdown, which is why this stays
        // `None` rather than becoming a zero that would read as "did no
        // reasoning".
        reasoning_tokens: usage.get("reasoning").and_then(Value::as_i64),
        // Pi reports no context window in its usage block.
        context_window: None,
        cost_usd: usage
            .get("cost")
            .and_then(|cost| cost.get("total"))
            .and_then(Value::as_f64),
    })
}

/// Tool calls in one reply that changed a file.
///
/// Pi's file tools are `write` (create or overwrite) and `edit` (exact text
/// replacement, possibly several disjoint ones in a single call). Anything
/// written through `bash` leaves no structured record, the same lower-bound
/// caveat every other source carries.
fn collect_edits(
    ctx: &SourceContext,
    message: &Value,
    timestamp: &str,
    event_key: &str,
    turn_id: Option<String>,
    cwd: Option<&str>,
    edits: &mut Vec<EditFact>,
) {
    let Some(blocks) = message.get("content").and_then(Value::as_array) else {
        return;
    };

    for (index, block) in blocks.iter().enumerate() {
        if block.get("type").and_then(Value::as_str) != Some("toolCall") {
            continue;
        }
        let name = block.get("name").and_then(Value::as_str).unwrap_or("");
        let Some(arguments) = block.get("arguments") else {
            continue;
        };
        let Some(path) = string_field(arguments, "path") else {
            continue;
        };

        let (op, added, removed) = match name {
            "write" => {
                let lines = arguments
                    .get("content")
                    .and_then(Value::as_str)
                    .map(count_lines)
                    .unwrap_or(0);
                (EditOp::Add, lines, 0)
            }
            "edit" => {
                // One call covers several disjoint replacements in the same
                // file, so the counts are the sum rather than the first.
                let (mut added, mut removed) = (0, 0);
                let entries = arguments
                    .get("edits")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for entry in &entries {
                    let (entry_added, entry_removed) = replacement_lines(entry);
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
            provider: "pi".to_string(),
            turn_id: turn_id.clone(),
            occurred_at: timestamp.to_string(),
            workspace: cwd.map(str::to_string),
            path,
            op,
            lines_added: Some(added),
            lines_removed: Some(removed),
        });
    }
}

/// Lines added and removed by one exact-text replacement.
fn replacement_lines(entry: &Value) -> (i64, i64) {
    let old = entry
        .get("oldText")
        .and_then(Value::as_str)
        .map(count_lines)
        .unwrap_or(0);
    let new = entry
        .get("newText")
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
        SourceContext::new("agent-1", "pi", std::path::Path::new("session.jsonl"))
    }

    fn parse(delta: &str) -> ParsedFacts {
        parse_delta(&ctx(), delta, SourceCarry::default())
    }

    const SESSION: &str = r#"{"type":"session","version":3,"id":"sess-1","timestamp":"2026-08-24T04:33:38.833Z","cwd":"D:\\Development\\Wardian"}"#;
    const MODEL_CHANGE: &str = r#"{"type":"model_change","id":"e6b9cb93","timestamp":"2026-08-24T04:33:38.906Z","provider":"openai-codex","modelId":"gpt-5.6-luna"}"#;
    const THINKING_LEVEL: &str = r#"{"type":"thinking_level_change","id":"ce0bc31a","timestamp":"2026-08-24T04:33:38.906Z","thinkingLevel":"medium"}"#;

    /// Transcribed from a real session: the record whose cache read is nonzero,
    /// which is the one that pins the disjointness claim.
    const ASSISTANT: &str = r#"{"type":"message","id":"3fe831af","timestamp":"2026-08-24T04:33:52.358Z","message":{"role":"assistant","api":"openai-codex-responses","provider":"openai-codex","model":"gpt-5.6-luna","responseId":"resp_1","stopReason":"stop","content":[{"type":"text","text":"done"}],"usage":{"input":2177,"output":7,"cacheRead":7680,"cacheWrite":0,"reasoning":0,"totalTokens":9864,"cost":{"input":0.0004354,"output":0.0000084,"cacheRead":0.0001536,"cacheWrite":0,"total":0.0005974}}}}"#;

    const USER: &str = r#"{"type":"message","id":"aab33156","timestamp":"2026-08-24T04:33:40.000Z","message":{"role":"user","content":[{"type":"text","text":"hi"}],"timestamp":1756000000000}}"#;

    fn assistant_with(content: &str) -> String {
        format!(
            r#"{{"type":"message","id":"tool-rec","timestamp":"2026-08-24T04:34:00.000Z","message":{{"role":"assistant","model":"gpt-5.6-luna","responseId":"resp_2","content":{content},"usage":{{"input":10,"output":2,"cacheRead":0,"cacheWrite":0,"totalTokens":12,"cost":{{"input":0.0,"output":0.0,"cacheRead":0.0,"cacheWrite":0.0,"total":0.0}}}}}}}}"#
        )
    }

    #[test]
    fn an_assistant_message_becomes_a_turn_with_split_tokens() {
        let facts = parse(&format!("{SESSION}\n{MODEL_CHANGE}\n{ASSISTANT}\n"));

        assert_eq!(facts.turns.len(), 1);
        let turn = &facts.turns[0];
        assert_eq!(turn.provider, "pi");
        assert_eq!(turn.input_tokens, Some(2177));
        assert_eq!(turn.cached_input_tokens, Some(7680));
        assert_eq!(turn.cache_write_tokens, Some(0));
        assert_eq!(turn.output_tokens, Some(7));
        assert_eq!(turn.reasoning_tokens, Some(0));
        assert_eq!(turn.model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(turn.turn_id.as_deref(), Some("resp_1"));
        assert_eq!(turn.cost_usd, Some(0.0005974));
    }

    #[test]
    fn pi_input_is_already_cache_exclusive_so_it_is_stored_unadjusted() {
        // The property that decides this parser's shape. Pi's own `totalTokens`
        // reconciles as `input + output + cacheRead`, which is only possible if
        // `input` excludes what was served from cache. Codex's subtraction
        // applied here would store 0 where 2,177 tokens were really read.
        let record: Value = serde_json::from_str(ASSISTANT).unwrap();
        let usage = &record["message"]["usage"];
        let declared = usage["totalTokens"].as_i64().unwrap();
        assert_eq!(
            declared,
            usage["input"].as_i64().unwrap()
                + usage["output"].as_i64().unwrap()
                + usage["cacheRead"].as_i64().unwrap()
        );

        let turn = &parse(&format!("{ASSISTANT}\n")).turns[0];
        assert_eq!(turn.input_tokens, Some(2177));
        assert_ne!(turn.input_tokens, Some(0));
    }

    #[test]
    fn pi_reasoning_stays_inside_output() {
        // Upstream documents `reasoning` as a subset of `output`. A record with
        // nonzero reasoning still reconciles without it, so nothing may add the
        // two together.
        const WITH_REASONING: &str = r#"{"type":"message","id":"r1","timestamp":"2026-08-24T04:33:52.358Z","message":{"role":"assistant","model":"gpt-5.6-luna","content":[],"usage":{"input":8586,"output":76,"cacheRead":0,"cacheWrite":0,"reasoning":35,"totalTokens":8662,"cost":{"input":0.0017172,"output":0.0000912,"cacheRead":0,"cacheWrite":0,"total":0.0018084}}}}"#;
        let record: Value = serde_json::from_str(WITH_REASONING).unwrap();
        let usage = &record["message"]["usage"];
        assert_eq!(
            usage["totalTokens"].as_i64().unwrap(),
            usage["input"].as_i64().unwrap() + usage["output"].as_i64().unwrap()
        );
        assert!(usage["reasoning"].as_i64().unwrap() > 0);

        let turn = &parse(&format!("{WITH_REASONING}\n")).turns[0];
        assert_eq!(turn.reasoning_tokens, Some(35));
        assert_eq!(turn.output_tokens, Some(76));
    }

    #[test]
    fn a_provider_without_a_reasoning_breakdown_reports_none_not_zero() {
        const NO_REASONING: &str = r#"{"type":"message","id":"n1","timestamp":"2026-08-24T04:33:52.358Z","message":{"role":"assistant","model":"m","content":[],"usage":{"input":5,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":6,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}}}}"#;
        let turn = &parse(&format!("{NO_REASONING}\n")).turns[0];
        assert_eq!(turn.reasoning_tokens, None);
    }

    #[test]
    fn pi_costs_reconcile_to_their_components() {
        // The assertion a future price table rests on: pi prices each component
        // separately and its total is exactly their sum, so per-token rates can
        // be recovered by division instead of transcribed from a vendor page.
        let record: Value = serde_json::from_str(ASSISTANT).unwrap();
        let cost = &record["message"]["usage"]["cost"];
        let summed = cost["input"].as_f64().unwrap()
            + cost["output"].as_f64().unwrap()
            + cost["cacheRead"].as_f64().unwrap()
            + cost["cacheWrite"].as_f64().unwrap();
        assert!((summed - cost["total"].as_f64().unwrap()).abs() < 1e-12);
    }

    #[test]
    fn only_assistant_messages_become_turns() {
        let facts = parse(&format!("{USER}\n{ASSISTANT}\n"));
        assert_eq!(facts.turns.len(), 1);
        // Both are still events: a user message is time the session was active.
        assert_eq!(facts.event_times.len(), 2);
    }

    #[test]
    fn the_session_header_supplies_the_workspace_for_later_edits() {
        let content = r#"[{"type":"toolCall","id":"c1","name":"write","arguments":{"path":"src/new.rs","content":"a\nb\n"}}]"#;
        let facts = parse(&format!("{SESSION}\n{}\n", assistant_with(content)));

        assert_eq!(facts.edits.len(), 1);
        let edit = &facts.edits[0];
        assert_eq!(edit.workspace.as_deref(), Some("D:\\Development\\Wardian"));
        assert_eq!(edit.path, "src/new.rs");
        assert_eq!(edit.op, EditOp::Add);
        assert_eq!(edit.lines_added, Some(2));
        assert_eq!(edit.lines_removed, Some(0));
    }

    #[test]
    fn an_edit_call_sums_its_disjoint_replacements() {
        let content = r#"[{"type":"toolCall","id":"c1","name":"edit","arguments":{"path":"src/lib.rs","edits":[{"oldText":"one\ntwo\n","newText":"uno\n"},{"oldText":"three\n","newText":"tres\ncuatro\n"}]}}]"#;
        let facts = parse(&format!("{SESSION}\n{}\n", assistant_with(content)));

        assert_eq!(facts.edits.len(), 1);
        let edit = &facts.edits[0];
        assert_eq!(edit.op, EditOp::Update);
        assert_eq!(edit.lines_added, Some(3));
        assert_eq!(edit.lines_removed, Some(3));
    }

    #[test]
    fn tool_calls_that_touch_no_file_are_not_edits() {
        let content = r#"[{"type":"toolCall","id":"c1","name":"read","arguments":{"path":"src/lib.rs"}},{"type":"toolCall","id":"c2","name":"bash","arguments":{"command":"ls"}}]"#;
        let facts = parse(&format!("{SESSION}\n{}\n", assistant_with(content)));
        assert!(facts.edits.is_empty());
        assert_eq!(facts.turns.len(), 1);
    }

    #[test]
    fn two_tool_calls_in_one_record_get_distinct_keys() {
        let content = r#"[{"type":"toolCall","id":"c1","name":"write","arguments":{"path":"a.rs","content":"a\n"}},{"type":"toolCall","id":"c2","name":"write","arguments":{"path":"b.rs","content":"b\n"}}]"#;
        let facts = parse(&format!("{SESSION}\n{}\n", assistant_with(content)));

        assert_eq!(facts.edits.len(), 2);
        assert_ne!(facts.edits[0].event_key, facts.edits[1].event_key);
    }

    #[test]
    fn the_thinking_level_becomes_the_turns_effort() {
        let facts = parse(&format!("{SESSION}\n{THINKING_LEVEL}\n{ASSISTANT}\n"));
        assert_eq!(facts.turns[0].effort.as_deref(), Some("medium"));
    }

    #[test]
    fn context_carries_across_a_delta_boundary() {
        // The header, the model, and the thinking level all land in the first
        // delta; the message that inherits them lands in the second. Without the
        // carry, the same record would parse differently depending on when
        // ingest ran.
        let first = parse(&format!("{SESSION}\n{MODEL_CHANGE}\n{THINKING_LEVEL}\n"));
        assert!(first.turns.is_empty());

        let second = parse_delta(&ctx(), &format!("{ASSISTANT}\n"), first.carry);
        let turn = &second.turns[0];
        assert_eq!(turn.model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(turn.effort.as_deref(), Some("medium"));
        assert_eq!(
            second.edits.first().map(|edit| edit.workspace.clone()),
            None
        );
        assert_eq!(second.carry.cwd.as_deref(), Some("D:\\Development\\Wardian"));
    }

    #[test]
    fn a_records_key_does_not_move_with_the_delta_boundary() {
        // Keys are the dedup identity. If one changed with where a delta was
        // cut, a re-read would double count rather than no-op.
        let whole = parse(&format!("{SESSION}\n{ASSISTANT}\n"));
        let prefix = format!("{SESSION}\n");
        let split = parse_delta_at(
            &ctx(),
            &format!("{ASSISTANT}\n"),
            prefix.len() as i64,
            SourceCarry::default(),
        );
        assert_eq!(whole.turns[0].event_key, split.turns[0].event_key);
    }

    #[test]
    fn a_partial_trailing_record_is_not_parsed() {
        let facts = parse(&format!("{SESSION}\n{{\"type\":\"mess"));
        assert!(facts.turns.is_empty());
    }

    #[test]
    fn unstamped_records_do_not_feed_clustering() {
        let facts = parse("{\"type\":\"session_info\",\"id\":\"x\",\"name\":\"n\"}\n");
        assert!(facts.event_times.is_empty());
    }

    #[test]
    fn a_message_without_usage_is_not_a_turn() {
        const NO_USAGE: &str = r#"{"type":"message","id":"u1","timestamp":"2026-08-24T04:33:52.358Z","message":{"role":"assistant","model":"m","content":[]}}"#;
        let facts = parse(&format!("{NO_USAGE}\n"));
        assert!(facts.turns.is_empty());
        assert_eq!(facts.event_times.len(), 1);
    }
}

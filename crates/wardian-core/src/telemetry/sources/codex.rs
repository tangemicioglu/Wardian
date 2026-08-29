//! Codex rollout log source.
//!
//! Codex writes `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, one JSON object
//! per line, each carrying a top-level `timestamp`. The records that matter:
//!
//! - `event_msg` / `token_count` — `info.last_token_usage` is the delta for the
//!   model call that just finished, and `info.total_token_usage` is the
//!   session-cumulative gauge. Only the delta is summed; summing the cumulative
//!   figure multiplies the true total by roughly the number of calls.
//! - `turn_context` — `turn_id`, `model`, `effort`, `cwd`.
//! - `event_msg` / `patch_apply_end` — a `changes` map keyed by absolute path.
//! - `token_count` / `rate_limits` — account-level limit gauge.
//!
//! Edit coverage is a lower bound and was measured, not assumed: across 57
//! August logs, all 59 `apply_patch` invocations produced `patch_apply_end`
//! records (1:1), but a further 17 `exec` calls wrote files through shell
//! redirects or interpreter one-liners that leave no patch record at all.

use crate::telemetry::identity::{content_key, file_fingerprint};
use crate::telemetry::models::{
    Cursor, CursorKind, EditFact, EditOp, LimitObservation, ParsedFacts, SourceCarry, SourceKind,
    TurnFact,
};
use crate::telemetry::sources::{SourceContext, SourceError, TelemetrySource};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};

#[derive(Debug, Default)]
pub struct CodexSource;

impl TelemetrySource for CodexSource {
    fn provider(&self) -> &'static str {
        "codex"
    }

    /// 2: `input_tokens` became cache-exclusive. Version 1 stored codex's raw
    /// figure, which includes cache reads, so every store written by it
    /// overstates token usage and must be re-read rather than left in place.
    fn parser_version(&self) -> i64 {
        2
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
        // The delta's own start, so each record's key can carry its absolute
        // position in the file rather than its position within this read.
        let base_offset = next_offset - delta.len() as i64;
        let facts = parse_delta_at(ctx, &delta, base_offset, carry);
        Ok((facts, Cursor::new(CursorKind::ByteOffset, next_offset)))
    }

    /// A byte offset only means something against the bytes it was taken from.
    fn cursor_is_stale(&self, ctx: &SourceContext, stored_fingerprint: Option<&str>) -> bool {
        match (stored_fingerprint, file_fingerprint(&ctx.path)) {
            (Some(stored), Some(current)) => stored != current,
            // Known identity, now unreadable. That happens when the file was
            // replaced and its first line is still being written, so resuming
            // from the old offset would consume the replacement from the middle.
            // Re-reading a file that merely became briefly unreadable is cheap;
            // silently losing a session's opening records is not.
            (Some(_), None) => true,
            // Never fingerprinted: either state predating this check, or a first
            // line too long for the window. The length comparison in
            // `read_delta` remains the fallback, which catches a shorter
            // replacement but not a longer one.
            (None, _) => false,
        }
    }
}

/// Read from `offset` to EOF, stopping at the last complete line.
///
/// A trailing partial line is left unconsumed so the next cycle re-reads it
/// whole; parsing a half-written record would either fail or, worse, succeed
/// against truncated JSON.
pub(crate) fn read_delta(
    path: &std::path::Path,
    offset: i64,
) -> Result<(String, i64), SourceError> {
    let mut file =
        std::fs::File::open(path).map_err(|err| SourceError::Unavailable(err.to_string()))?;
    let size = file
        .metadata()
        .map_err(|err| SourceError::Read(err.to_string()))?
        .len() as i64;

    // The file was truncated or replaced, so the old position points into
    // unrelated bytes. Start over rather than emit garbage.
    let start = if size < offset { 0 } else { offset };
    if size == start {
        return Ok((String::new(), start));
    }

    file.seek(SeekFrom::Start(start as u64))
        .map_err(|err| SourceError::Read(err.to_string()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| SourceError::Read(err.to_string()))?;

    let consumed = match bytes.iter().rposition(|byte| *byte == b'\n') {
        Some(index) => index + 1,
        // No complete line yet; hold position until one lands.
        None => return Ok((String::new(), start)),
    };
    let text = String::from_utf8_lossy(&bytes[..consumed]).into_owned();
    Ok((text, start + consumed as i64))
}

/// Pure parse of a complete-line delta. No filesystem, no database.
///
/// `carry` seeds the turn context from the previous delta. `token_count`
/// records hold no turn identity of their own and inherit the most recent
/// `turn_context`, which is frequently in an earlier delta — so starting each
/// delta empty would attribute the same record differently depending on when
/// ingest ran, and would change its dedup key with it.
pub fn parse_delta(ctx: &SourceContext, delta: &str, carry: SourceCarry) -> ParsedFacts {
    parse_delta_at(ctx, delta, 0, carry)
}

/// As [`parse_delta`], but told where in the file the delta begins.
///
/// The offset makes each record's key positional as well as content-derived. A
/// content hash alone would merge two byte-identical records into one, and a
/// 64-bit digest also leaves a small collision probability that silently drops a
/// fact. Two records cannot share an offset, so the pair is unique by
/// construction; the hash then guards the one case offsets do not cover, which
/// is a file rewritten so that a new record occupies an old record's position.
pub fn parse_delta_at(
    ctx: &SourceContext,
    delta: &str,
    base_offset: i64,
    carry: SourceCarry,
) -> ParsedFacts {
    let mut facts = ParsedFacts::default();
    let mut current_turn = carry.turn_id;
    let mut current_model = carry.model;
    let mut current_effort = carry.effort;
    let mut current_cwd = carry.cwd;

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

        let timestamp = record.get("timestamp").and_then(Value::as_str);
        if let Some(timestamp) = timestamp {
            facts.event_times.push(timestamp.to_string());
        }

        let record_type = record.get("type").and_then(Value::as_str).unwrap_or("");
        let payload = record.get("payload");
        // Absolute position plus content digest. Both halves are identical
        // whether this line arrives in a delta or in a full re-read, because the
        // offset is measured from the start of the file rather than the start of
        // the read.
        let event_key = format!("{record_offset:012x}:{}", content_key(trimmed.as_bytes()));

        if record_type == "turn_context" {
            if let Some(payload) = payload {
                current_turn = string_field(payload, "turn_id");
                current_model = string_field(payload, "model").or_else(|| {
                    payload
                        .get("collaboration_mode")
                        .and_then(|mode| mode.get("settings"))
                        .and_then(|settings| string_field(settings, "model"))
                });
                current_effort = string_field(payload, "effort");
                current_cwd = string_field(payload, "cwd");
            }
            continue;
        }

        let Some(payload) = payload else { continue };
        match payload.get("type").and_then(Value::as_str).unwrap_or("") {
            "token_count" => {
                let Some(timestamp) = timestamp else { continue };
                if let Some(turn) = parse_token_count(
                    ctx,
                    payload,
                    timestamp,
                    &event_key,
                    current_turn.clone(),
                    current_model.clone(),
                    current_effort.clone(),
                ) {
                    facts.turns.push(turn);
                }
                if let Some(limit) = parse_rate_limits(ctx, payload, timestamp) {
                    facts.limits.push(limit);
                }
            }
            "patch_apply_end" => {
                let Some(timestamp) = timestamp else { continue };
                parse_patch_apply(
                    ctx,
                    payload,
                    timestamp,
                    &event_key,
                    current_turn.clone(),
                    current_cwd.as_deref(),
                    &mut facts.edits,
                );
            }
            _ => {}
        }
    }

    facts.carry = SourceCarry {
        turn_id: current_turn,
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
        .filter(|found| !found.is_empty() && found != "None")
}

#[allow(clippy::too_many_arguments)]
fn parse_token_count(
    ctx: &SourceContext,
    payload: &Value,
    timestamp: &str,
    event_key: &str,
    turn_id: Option<String>,
    model: Option<String>,
    effort: Option<String>,
) -> Option<TurnFact> {
    let info = payload.get("info")?;
    // The per-call delta, never the cumulative gauge beside it.
    let usage = info.get("last_token_usage")?;
    fn fresh_input_tokens(usage: &Value) -> Option<i64> {
        let input = usage.get("input_tokens").and_then(Value::as_i64)?;
        let cached = usage
            .get("cached_input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        // Clamped because a negative token count is not a meaningful quantity,
        // and a provider that ever reported cached > input would otherwise
        // subtract real usage away from the habitat total.
        Some((input - cached).max(0))
    }

    Some(TurnFact {
        event_key: event_key.to_string(),
        session_id: ctx.session_id.clone(),
        provider: "codex".to_string(),
        turn_id,
        model,
        effort,
        started_at: None,
        ended_at: timestamp.to_string(),
        // Normalized to *fresh* input: codex's `input_tokens` is the whole
        // prompt including whatever was served from cache, so storing it raw
        // makes `input + output` count every cache read twice. Measured on a
        // real habitat that overstated usage 49-fold, because cache reads run
        // several times fresh input and grow with conversation length.
        //
        // Opencode reports the two disjointly (`total = input + output +
        // cache.read + cache.write`), so it needs no adjustment. Normalizing
        // here rather than at read time is what stops one column meaning two
        // different things depending on which provider filled it.
        input_tokens: fresh_input_tokens(usage),
        cached_input_tokens: usage.get("cached_input_tokens").and_then(Value::as_i64),
        cache_write_tokens: usage
            .get("cache_write_input_tokens")
            .and_then(Value::as_i64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_i64),
        reasoning_tokens: usage.get("reasoning_output_tokens").and_then(Value::as_i64),
        context_window: info.get("model_context_window").and_then(Value::as_i64),
        // Codex reports rate-limit headroom instead of currency.
        cost_usd: None,
    })
}

fn parse_rate_limits(
    ctx: &SourceContext,
    payload: &Value,
    timestamp: &str,
) -> Option<LimitObservation> {
    let limits = payload.get("rate_limits")?;
    let primary = limits.get("primary")?;
    if primary.is_null() {
        return None;
    }

    let resets_at = primary
        .get("resets_at")
        .and_then(Value::as_i64)
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));

    Some(LimitObservation {
        provider: ctx.provider.clone(),
        limit_id: string_field(limits, "limit_id"),
        observed_at: timestamp.to_string(),
        used_percent: primary.get("used_percent").and_then(Value::as_f64),
        window_minutes: primary.get("window_minutes").and_then(Value::as_i64),
        resets_at,
        plan_type: string_field(limits, "plan_type"),
    })
}

fn parse_patch_apply(
    ctx: &SourceContext,
    payload: &Value,
    timestamp: &str,
    event_key: &str,
    carried_turn_id: Option<String>,
    cwd: Option<&str>,
    edits: &mut Vec<EditFact>,
) {
    // A failed patch changed nothing on disk and must not be counted.
    if payload.get("success").and_then(Value::as_bool) == Some(false) {
        return;
    }
    let turn_id = string_field(payload, "turn_id").or(carried_turn_id);
    let Some(changes) = payload.get("changes").and_then(Value::as_object) else {
        return;
    };

    for (path, spec) in changes {
        let kind = spec.get("type").and_then(Value::as_str).unwrap_or("");
        let (op, lines_added, lines_removed) = match kind {
            "add" => {
                let added = spec
                    .get("content")
                    .and_then(Value::as_str)
                    .map(count_lines)
                    .unwrap_or(0);
                (EditOp::Add, Some(added), Some(0))
            }
            "update" => match spec.get("unified_diff").and_then(Value::as_str) {
                Some(diff) => {
                    let (added, removed) = count_diff_lines(diff);
                    (EditOp::Update, Some(added), Some(removed))
                }
                None => (EditOp::Update, None, None),
            },
            "delete" => (EditOp::Delete, Some(0), None),
            _ => continue,
        };

        edits.push(EditFact {
            // One record can change several files, so the path distinguishes
            // them within it.
            event_key: format!("{event_key}:{path}"),
            session_id: ctx.session_id.clone(),
            provider: "codex".to_string(),
            turn_id: turn_id.clone(),
            occurred_at: timestamp.to_string(),
            workspace: cwd.map(str::to_string),
            path: path.clone(),
            op,
            lines_added,
            lines_removed,
        });
    }
}

fn count_lines(content: &str) -> i64 {
    if content.is_empty() {
        return 0;
    }
    content.lines().count() as i64
}

/// Count added and removed lines in a unified diff hunk.
///
/// `+++` and `---` are file headers rather than content, so they are excluded.
fn count_diff_lines(diff: &str) -> (i64, i64) {
    let mut added = 0;
    let mut removed = 0;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if let Some(first) = line.chars().next() {
            match first {
                '+' => added += 1,
                '-' => removed += 1,
                _ => {}
            }
        }
    }
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ctx() -> SourceContext {
        SourceContext::new("agent-1", "codex", std::path::Path::new("rollout.jsonl"))
    }

    // Verbatim shapes captured from live codex logs on 2026-08-13.
    const TOKEN_COUNT: &str = r#"{"timestamp":"2026-08-13T18:42:49.628Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":831424,"cached_input_tokens":730880,"cache_write_input_tokens":0,"output_tokens":5254,"reasoning_output_tokens":2244,"total_tokens":836678},"last_token_usage":{"input_tokens":92854,"cached_input_tokens":91904,"cache_write_input_tokens":0,"output_tokens":71,"reasoning_output_tokens":53,"total_tokens":92925},"model_context_window":258400},"rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":43.0,"window_minutes":10080,"resets_at":1787197017},"secondary":null,"plan_type":"prolite"}}}"#;

    const TURN_CONTEXT: &str = r#"{"timestamp":"2026-08-13T18:40:46.104Z","type":"turn_context","payload":{"turn_id":"019ffc6d-3a37-7801-9936-be6d4356cece","cwd":"D:\\Development\\Wardian","model":"gpt-5.6-terra","effort":"high"}}"#;

    const PATCH_ADD: &str = r#"{"timestamp":"2026-08-13T18:42:43.883Z","type":"event_msg","payload":{"type":"patch_apply_end","turn_id":"t-1","success":true,"changes":{"D:\\repo\\review.md":{"type":"add","content":"one\ntwo\nthree"}}}}"#;

    const PATCH_UPDATE: &str = r#"{"timestamp":"2026-08-13T18:43:00.000Z","type":"event_msg","payload":{"type":"patch_apply_end","turn_id":"t-1","success":true,"changes":{"D:\\repo\\notes.md":{"type":"update","unified_diff":"@@ -2,2 +2,10 @@\n \n+added one\n+added two\n-removed one\n","move_path":"None"}}}}"#;

    #[test]
    fn token_count_uses_the_delta_not_the_cumulative_gauge() {
        let facts = parse_delta(&ctx(), TOKEN_COUNT, SourceCarry::default());
        assert_eq!(facts.turns.len(), 1);
        let turn = &facts.turns[0];
        // last_token_usage, not total_token_usage (831424) — and normalized to
        // fresh input, so 92854 raw prompt minus 91904 served from cache.
        // Storing the raw 92854 would count those 91904 twice as soon as
        // anything summed input alongside cached.
        assert_eq!(turn.input_tokens, Some(950));
        assert_eq!(turn.cached_input_tokens, Some(91904));
        assert_eq!(turn.output_tokens, Some(71));
        assert_eq!(turn.reasoning_tokens, Some(53));
        assert_eq!(turn.context_window, Some(258400));
    }

    #[test]
    fn cached_tokens_stay_separate_from_input() {
        let facts = parse_delta(&ctx(), TOKEN_COUNT, SourceCarry::default());
        let turn = &facts.turns[0];

        // Codex's raw `input_tokens` is the whole prompt *including* the part
        // served from cache. This record reports 92854 raw against 91904
        // cached, so all but 950 tokens of that prompt were a cache hit.
        // Storing the raw figure — which this test previously asserted, while
        // its comment claimed the opposite — makes `input + output` count the
        // cache reads a second time.
        assert_eq!(turn.input_tokens, Some(950));
        assert_eq!(turn.cached_input_tokens, Some(91904));

        // The two are now disjoint, so they add back up to what codex billed.
        assert_eq!(
            turn.input_tokens.unwrap() + turn.cached_input_tokens.unwrap(),
            92854
        );
    }

    #[test]
    fn a_fully_cached_prompt_reports_no_fresh_input() {
        // The degenerate end of the same rule: a prompt served entirely from
        // cache did process new input of zero, which is a real reading and not
        // the same as the provider having reported nothing.
        let record = r#"{"timestamp":"2026-08-13T18:42:49.628Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":4096,"cached_input_tokens":4096,"output_tokens":12}}}}"#;
        let facts = parse_delta(&ctx(), record, SourceCarry::default());
        let turn = &facts.turns[0];
        assert_eq!(turn.input_tokens, Some(0));
        assert_eq!(turn.cached_input_tokens, Some(4096));
    }

    #[test]
    fn a_record_without_cache_accounting_keeps_its_input_intact() {
        // No `cached_input_tokens` means nothing was cached, not that the whole
        // prompt was. Subtracting a default of anything but zero here would
        // quietly delete real usage.
        let record = r#"{"timestamp":"2026-08-13T18:42:49.628Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":4096,"output_tokens":12}}}}"#;
        let facts = parse_delta(&ctx(), record, SourceCarry::default());
        assert_eq!(facts.turns[0].input_tokens, Some(4096));
    }

    #[test]
    fn turn_context_attributes_the_following_token_count() {
        let delta = format!("{TURN_CONTEXT}\n{TOKEN_COUNT}\n");
        let facts = parse_delta(&ctx(), &delta, SourceCarry::default());
        let turn = &facts.turns[0];
        assert_eq!(turn.model.as_deref(), Some("gpt-5.6-terra"));
        assert_eq!(turn.effort.as_deref(), Some("high"));
        assert_eq!(
            turn.turn_id.as_deref(),
            Some("019ffc6d-3a37-7801-9936-be6d4356cece")
        );
    }

    #[test]
    fn token_count_without_preceding_context_is_unattributed_not_guessed() {
        let facts = parse_delta(&ctx(), TOKEN_COUNT, SourceCarry::default());
        assert_eq!(facts.turns[0].model, None);
    }

    #[test]
    fn rate_limits_are_captured_with_reset_time() {
        let facts = parse_delta(&ctx(), TOKEN_COUNT, SourceCarry::default());
        assert_eq!(facts.limits.len(), 1);
        let limit = &facts.limits[0];
        assert_eq!(limit.used_percent, Some(43.0));
        assert_eq!(limit.window_minutes, Some(10080));
        assert_eq!(limit.plan_type.as_deref(), Some("prolite"));
        assert!(limit.resets_at.is_some());
    }

    #[test]
    fn added_file_counts_its_lines() {
        let facts = parse_delta(&ctx(), PATCH_ADD, SourceCarry::default());
        assert_eq!(facts.edits.len(), 1);
        let edit = &facts.edits[0];
        assert_eq!(edit.op, EditOp::Add);
        assert_eq!(edit.lines_added, Some(3));
        assert_eq!(edit.lines_removed, Some(0));
    }

    #[test]
    fn updated_file_counts_diff_lines() {
        let facts = parse_delta(&ctx(), PATCH_UPDATE, SourceCarry::default());
        let edit = &facts.edits[0];
        assert_eq!(edit.op, EditOp::Update);
        assert_eq!(edit.lines_added, Some(2));
        assert_eq!(edit.lines_removed, Some(1));
    }

    #[test]
    fn diff_file_headers_are_not_counted_as_content() {
        let (added, removed) = count_diff_lines("--- a/x\n+++ b/x\n@@ -1 +1 @@\n+new\n-old\n");
        assert_eq!((added, removed), (1, 1));
    }

    #[test]
    fn failed_patch_is_not_counted() {
        let line = r#"{"timestamp":"2026-08-13T18:42:43.883Z","type":"event_msg","payload":{"type":"patch_apply_end","success":false,"changes":{"x.md":{"type":"add","content":"a"}}}}"#;
        assert!(parse_delta(&ctx(), line, SourceCarry::default())
            .edits
            .is_empty());
    }

    #[test]
    fn workspace_comes_from_turn_context_cwd() {
        let delta = format!("{TURN_CONTEXT}\n{PATCH_ADD}\n");
        let facts = parse_delta(&ctx(), &delta, SourceCarry::default());
        assert_eq!(
            facts.edits[0].workspace.as_deref(),
            Some("D:\\Development\\Wardian")
        );
    }

    #[test]
    fn every_timestamped_record_feeds_clustering() {
        let delta = format!("{TURN_CONTEXT}\n{TOKEN_COUNT}\n{PATCH_ADD}\n");
        let facts = parse_delta(&ctx(), &delta, SourceCarry::default());
        assert_eq!(facts.event_times.len(), 3);
    }

    #[test]
    fn codex_emits_no_measured_intervals() {
        // Codex cannot report durations, so it must feed the clusterer only.
        let facts = parse_delta(&ctx(), TOKEN_COUNT, SourceCarry::default());
        assert!(facts.intervals.is_empty());
        assert!(!facts.event_times.is_empty());
    }

    #[test]
    fn malformed_lines_are_skipped_not_fatal() {
        let delta = format!("not json\n{TOKEN_COUNT}\n{{\"partial\":\n");
        let facts = parse_delta(&ctx(), &delta, SourceCarry::default());
        assert_eq!(facts.turns.len(), 1);
    }

    // --- delta reading ---

    fn temp_log(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(&path, contents).unwrap();
        (dir, path)
    }

    #[test]
    fn delta_reads_only_new_bytes() {
        let (_dir, path) = temp_log("alpha\nbeta\n");
        let (first, offset) = read_delta(&path, 0).unwrap();
        assert_eq!(first, "alpha\nbeta\n");
        assert_eq!(offset, 11);

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"gamma\n").unwrap();
        let (second, next) = read_delta(&path, offset).unwrap();
        assert_eq!(second, "gamma\n");
        assert_eq!(next, 17);
    }

    #[test]
    fn trailing_partial_line_is_left_for_next_cycle() {
        let (_dir, path) = temp_log("alpha\nbeta");
        let (delta, offset) = read_delta(&path, 0).unwrap();
        assert_eq!(delta, "alpha\n");
        // "beta" is unconsumed until its newline arrives.
        assert_eq!(offset, 6);
    }

    #[test]
    fn a_file_with_no_complete_line_holds_position() {
        let (_dir, path) = temp_log("partial");
        let (delta, offset) = read_delta(&path, 0).unwrap();
        assert!(delta.is_empty());
        assert_eq!(offset, 0);
    }

    #[test]
    fn truncation_restarts_from_the_beginning() {
        let (_dir, path) = temp_log("alpha\nbeta\n");
        let (_, offset) = read_delta(&path, 0).unwrap();
        // Rotated: the file is now shorter than our recorded position.
        std::fs::write(&path, "fresh\n").unwrap();
        let (delta, next) = read_delta(&path, offset).unwrap();
        assert_eq!(delta, "fresh\n");
        assert_eq!(next, 6);
    }

    #[test]
    fn unchanged_file_yields_nothing() {
        let (_dir, path) = temp_log("alpha\n");
        let (_, offset) = read_delta(&path, 0).unwrap();
        let (delta, next) = read_delta(&path, offset).unwrap();
        assert!(delta.is_empty());
        assert_eq!(next, offset);
    }

    #[test]
    fn missing_file_is_unavailable_not_a_panic() {
        let err = read_delta(std::path::Path::new("does-not-exist.jsonl"), 0).unwrap_err();
        assert!(matches!(err, SourceError::Unavailable(_)));
    }
}

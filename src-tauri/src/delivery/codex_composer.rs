use std::sync::{Arc, Mutex};

use crate::state::{AgentWatchState, AppState};
use crate::utils::delivery_transaction::TerminalDeliveryError;
use crate::utils::strip_ansi_controls;

const PAYLOAD_APPLY_TIMEOUT_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationScope {
    TransactionDelta,
    ActivePromptFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposerObservation {
    literal_match_bytes: usize,
    normalized_payload_bytes: usize,
    marker_format: &'static str,
    marker_chars: Option<usize>,
    codex_version: Option<String>,
    source: &'static str,
}

impl ComposerObservation {
    fn confirms_payload(&self) -> bool {
        self.marker_chars.is_some()
            || (self.normalized_payload_bytes > 0
                && self.literal_match_bytes == self.normalized_payload_bytes)
    }

    fn observed_state(&self) -> String {
        format!(
            "literal_match_bytes={};normalized_payload_bytes={};marker_format={};marker_chars={};codex_version={};observation_source={}",
            self.literal_match_bytes,
            self.normalized_payload_bytes,
            self.marker_format,
            self.marker_chars
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.codex_version.as_deref().unwrap_or("unknown"),
            self.source,
        )
    }
}

async fn terminal_output_snapshot(state: &AppState, session_id: &str) -> Result<String, String> {
    let watch_state = {
        let agents = state.agents.lock().await;
        agents
            .get(session_id)
            .ok_or_else(|| format!("Agent {session_id} not found or is off"))?
            .watch_state
            .clone()
    };
    let snapshot = watch_state
        .lock()
        .map_err(|_| format!("Agent {session_id} watch state lock poisoned"))?
        .snapshot_since(None, None)
        .map(|snapshot| snapshot.output.text)
        .map_err(|error| format!("watch state error: {}", error.code()));
    snapshot
}

pub async fn session_has_stalled_composer(
    state: &AppState,
    session_id: &str,
) -> Result<bool, String> {
    terminal_output_snapshot(state, session_id)
        .await
        .map(|output| pending_paste_chars(&output).is_some())
}

/// Wait for Codex to prove that it has applied the bracketed paste to its
/// composer before Wardian sends Return. ConPTY's write receipt only proves
/// that bytes reached the PTY, not that Codex's event loop consumed them.
pub async fn wait_for_payload_applied_before_submit(
    state: &AppState,
    session_id: &str,
    since_cursor: &str,
    prompt: &str,
) -> Result<(), TerminalDeliveryError> {
    let watch_state = {
        let agents = state.agents.lock().await;
        agents
            .get(session_id)
            .ok_or_else(|| {
                TerminalDeliveryError::terminal_state_unknown(
                    "payload_apply_unconfirmed",
                    format!("Agent {session_id} not found or is off after payload write"),
                )
            })?
            .watch_state
            .clone()
    };
    wait_for_watch_payload_applied(watch_state, session_id, since_cursor, prompt).await
}

async fn wait_for_watch_payload_applied(
    watch_state: Arc<Mutex<AgentWatchState>>,
    session_id: &str,
    since_cursor: &str,
    prompt: &str,
) -> Result<(), TerminalDeliveryError> {
    let started = tokio::time::Instant::now();
    let mut best_observation = ComposerObservation {
        literal_match_bytes: 0,
        normalized_payload_bytes: normalize_echo_text(prompt).len(),
        marker_format: "absent",
        marker_chars: None,
        codex_version: None,
        source: "transaction_delta",
    };
    while started.elapsed() < std::time::Duration::from_millis(PAYLOAD_APPLY_TIMEOUT_MS) {
        let (output, scope) = {
            let watch_state = watch_state.lock().map_err(|_| {
                TerminalDeliveryError::terminal_state_unknown(
                    "payload_apply_unconfirmed",
                    format!("Agent {session_id} watch state lock poisoned after payload write"),
                )
            })?;
            // Do not tail-cap this transaction delta. Codex can emit enough
            // startup/repaint traffic to discard the collapsed-paste marker.
            // If churn expires the cursor, the delivery lock plus active-prompt
            // parser safely scope the retained composer fallback.
            match watch_state.snapshot_since(Some(since_cursor), None) {
                Ok(snapshot) => (snapshot.output.text, ObservationScope::TransactionDelta),
                Err(error) if error.code() == "cursor_expired" => watch_state
                    .snapshot_since(None, None)
                    .map(|snapshot| (snapshot.output.text, ObservationScope::ActivePromptFallback))
                    .map_err(|fallback_error| {
                        TerminalDeliveryError::terminal_state_unknown(
                            "payload_apply_unconfirmed",
                            format!(
                                "watch state fallback error after payload write: {}",
                                fallback_error.code()
                            ),
                        )
                    })?,
                Err(error) => {
                    return Err(TerminalDeliveryError::terminal_state_unknown(
                        "payload_apply_unconfirmed",
                        format!("watch state error after payload write: {}", error.code()),
                    ));
                }
            }
        };
        let observation = observe_payload_application(&output, prompt, scope);
        if observation.confirms_payload() {
            return Ok(());
        }
        if observation.literal_match_bytes > best_observation.literal_match_bytes
            || (best_observation.marker_format == "absent" && observation.marker_format != "absent")
        {
            best_observation = observation;
        } else if best_observation.codex_version.is_none() && observation.codex_version.is_some() {
            best_observation.codex_version = observation.codex_version;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    if best_observation.codex_version.is_none() {
        best_observation.codex_version =
            crate::providers::models::installed_provider_version("codex").await;
    }

    Err(TerminalDeliveryError::terminal_state_unknown(
        "payload_apply_unconfirmed",
        format!(
            "Timed out waiting for {session_id} Codex composer to apply the payload; Return was not sent"
        ),
    )
    .with_observation(
        best_observation.observed_state(),
        "Codex composer evidence remained incomplete; diagnostics are counts and provider format only and do not include prompt content",
    ))
}

pub fn output_has_ready_prompt(output: &str) -> bool {
    let cleaned = strip_ansi_controls(output).replace('\r', "\n");
    if output_has_workspace_trust_prompt(&cleaned) || pending_paste_chars(&cleaned).is_some() {
        return false;
    }
    let mut trailing_metadata_lines = 0usize;
    for line in cleaned.lines().rev().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if line.starts_with('›') {
            return true;
        }
        if trailing_metadata_lines < 3 && ready_prompt_trailing_metadata_line(line) {
            trailing_metadata_lines += 1;
            continue;
        }
        return false;
    }
    false
}

pub fn output_has_workspace_trust_prompt(output: &str) -> bool {
    output
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
        .contains("do you trust the contents of this directory?")
}

fn observe_payload_application(
    output: &str,
    prompt: &str,
    scope: ObservationScope,
) -> ComposerObservation {
    let cleaned = strip_ansi_controls(output).replace('\r', "\n");
    let observed = match scope {
        // The cursor is captured immediately before Wardian writes the payload,
        // so a marker in this delta belongs to this delivery even when Codex
        // repaints only composer cells and does not re-emit the prompt glyph.
        ObservationScope::TransactionDelta => cleaned.as_str(),
        ObservationScope::ActivePromptFallback => cleaned
            .rsplit_once('›')
            .map_or(cleaned.as_str(), |(_, tail)| tail),
    };
    let marker_chars = paste_marker_chars(observed);
    let normalized_observed = normalize_echo_text(observed);
    let token = normalize_echo_text(prompt);
    ComposerObservation {
        literal_match_bytes: longest_prefix_match_bytes(&normalized_observed, &token),
        normalized_payload_bytes: token.len(),
        marker_format: if marker_chars.is_some() {
            "pasted_content_chars"
        } else if marker_like_text(observed) {
            "unrecognized_marker_like"
        } else {
            "absent"
        },
        marker_chars,
        codex_version: codex_version(output),
        source: match scope {
            ObservationScope::TransactionDelta => "transaction_delta",
            ObservationScope::ActivePromptFallback => "active_prompt_fallback",
        },
    }
}

fn pending_paste_chars(output: &str) -> Option<usize> {
    let cleaned = strip_ansi_controls(output).replace('\r', "\n");
    let active_prompt = cleaned.rsplit_once('›')?.1;
    paste_marker_chars(active_prompt)
}

fn paste_marker_chars(output: &str) -> Option<usize> {
    const PREFIX: &str = "[Pasted Content ";
    const SUFFIX: &str = " chars]";

    // Cursor movement can wrap the marker between "Pasted" and "Content".
    let normalized = output.split_whitespace().collect::<Vec<_>>().join(" ");
    let marker_start = normalized.rfind(PREFIX)? + PREFIX.len();
    let remainder = &normalized[marker_start..];
    let marker_end = remainder.find(SUFFIX)?;
    remainder[..marker_end].trim().parse().ok()
}

fn marker_like_text(output: &str) -> bool {
    let normalized = output.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.contains("[Pasted") || normalized.contains("Pasted Content")
}

fn codex_version(output: &str) -> Option<String> {
    let marker = "OpenAI Codex (v";
    let start = output.rfind(marker)? + marker.len();
    let version = output[start..].split(')').next()?.trim();
    (!version.is_empty()
        && version
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-+".contains(character)))
    .then(|| version.to_string())
}

fn longest_prefix_match_bytes(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let pattern = needle.as_bytes();
    let mut prefix = vec![0usize; pattern.len()];
    for index in 1..pattern.len() {
        let mut matched = prefix[index - 1];
        while matched > 0 && pattern[index] != pattern[matched] {
            matched = prefix[matched - 1];
        }
        if pattern[index] == pattern[matched] {
            matched += 1;
        }
        prefix[index] = matched;
    }
    let mut matched = 0usize;
    let mut best = 0usize;
    for byte in haystack.bytes() {
        while matched > 0 && byte != pattern[matched] {
            matched = prefix[matched - 1];
        }
        if byte == pattern[matched] {
            matched += 1;
            best = best.max(matched);
            if matched == pattern.len() {
                return matched;
            }
        }
    }
    best
}

fn normalize_echo_text(text: &str) -> String {
    strip_ansi_controls(text)
        .replace('\r', "\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn ready_prompt_trailing_metadata_line(line: &str) -> bool {
    if line.contains('•') {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    lower.starts_with("gpt-") && (line.contains('·') || lower.contains("context"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_prompt_detects_visible_compose_prompt() {
        assert!(output_has_ready_prompt("\r\n› Write tests for @filename"));
        assert!(output_has_ready_prompt(
            "\r\n›\u{1b}[22m Write tests for @filename"
        ));
        assert!(output_has_ready_prompt(
            "\r\n› Explain this codebase\r\n\r\n  gpt-5.5 high · Context 100% left · C:\\projects\\example\r\n"
        ));
        assert!(!output_has_ready_prompt("Booting MCP server"));
    }

    #[test]
    fn ready_prompt_rejects_active_or_historical_collapsed_paste() {
        let active =
            "\r\n› [Pasted Content 6479 chars]\r\n\r\n  gpt-5.5 high · Context 49% left\r\n";
        assert!(!output_has_ready_prompt(active));
        assert_eq!(pending_paste_chars(active), Some(6479));

        let historical =
            "\r\n› [Pasted Content 6479 chars]\r\nresponse\r\n› Ask Codex to do anything\r\n";
        assert_eq!(pending_paste_chars(historical), None);
        assert!(output_has_ready_prompt(historical));
    }

    #[test]
    fn pending_paste_accepts_marker_wrapped_by_cursor_movement() {
        let output = "\r\n› visible prefix [Pasted\x1b[23;1H  Content 5865 chars]\r\n";
        assert_eq!(pending_paste_chars(output), Some(5865));
        assert!(observe_payload_application(
            output,
            &"x".repeat(7_000),
            ObservationScope::TransactionDelta
        )
        .confirms_payload());
    }

    #[test]
    fn payload_application_requires_complete_current_payload() {
        assert!(observe_payload_application(
            "\r\n› hello",
            "hello",
            ObservationScope::ActivePromptFallback
        )
        .confirms_payload());
        assert!(observe_payload_application(
            "\r\n› visible prefix [Pasted Content 5890 chars]",
            &"x".repeat(7_000),
            ObservationScope::ActivePromptFallback
        )
        .confirms_payload());
        assert!(!observe_payload_application(
            "\r\n› first line",
            "first line\nsecond line",
            ObservationScope::ActivePromptFallback
        )
        .confirms_payload());
        assert!(!observe_payload_application(
            "\r\n› hello\r\nresponse\r\n› Ask Codex to do anything",
            "hello",
            ObservationScope::ActivePromptFallback
        )
        .confirms_payload());
    }

    #[test]
    fn transaction_delta_accepts_cell_only_marker_repaint() {
        let observation = observe_payload_application(
            "\x1b[22;3H[Pasted Content 6323 chars]\x1b[K",
            &"x".repeat(7_000),
            ObservationScope::TransactionDelta,
        );

        assert!(observation.confirms_payload());
        assert_eq!(observation.marker_chars, Some(6323));
        assert_eq!(observation.source, "transaction_delta");
    }

    #[test]
    fn diagnostics_classify_unknown_marker_and_provider_version_without_content() {
        let observation = observe_payload_application(
            "OpenAI Codex (v0.151.0)\r\n\x1b[22;3H[Pasted text 6400 bytes]",
            "private payload",
            ObservationScope::TransactionDelta,
        );

        assert!(!observation.confirms_payload());
        assert_eq!(observation.marker_format, "unrecognized_marker_like");
        assert_eq!(observation.codex_version.as_deref(), Some("0.151.0"));
        assert!(!observation.observed_state().contains("private payload"));
    }

    #[test]
    fn diagnostics_report_partial_literal_match() {
        let observation = observe_payload_application(
            "\x1b[22;3Hfirst line second",
            "first line second line third",
            ObservationScope::TransactionDelta,
        );

        assert_eq!(observation.literal_match_bytes, "first line second".len());
        assert_eq!(
            observation.normalized_payload_bytes,
            "first line second line third".len()
        );
    }

    #[tokio::test]
    async fn payload_application_recovers_from_repaint_cursor_expiry() {
        let watch_state = Arc::new(Mutex::new(AgentWatchState::new(
            "agent-1".to_string(),
            16,
            262_144,
        )));
        let cursor = watch_state.lock().unwrap().latest_cursor();
        {
            let mut state = watch_state.lock().unwrap();
            for _ in 0..17 {
                state.push_output(b"\x1b[?2026h");
            }
            state.push_output(b"\r\n\xe2\x80\xba [Pasted Content 5890 chars]\r\n");
        }

        wait_for_watch_payload_applied(watch_state, "agent-1", &cursor, &"x".repeat(7_000))
            .await
            .expect("active composer evidence should survive cursor expiry");
    }

    #[test]
    fn ready_prompt_rejects_workspace_trust_or_busy_tail() {
        assert!(!output_has_ready_prompt(
            "\r\n› 1. Yes, continue\r\n  2. No, quit\r\nDo you trust the contents of this directory?\r\nPress enter to continue"
        ));
        for busy in [
            "Processing request",
            "Thinking about the request",
            "Final response: complete",
            "Final response: gpt-5 · context window",
        ] {
            assert!(!output_has_ready_prompt(&format!(
                "\r\n› Previous prompt\r\n{busy}\r\n"
            )));
        }
    }
}

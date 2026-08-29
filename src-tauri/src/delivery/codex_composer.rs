use std::sync::{Arc, Mutex};

use crate::state::{AgentWatchState, AppState};
use crate::utils::delivery_transaction::TerminalDeliveryError;
use crate::utils::strip_ansi_controls;

const PAYLOAD_APPLY_TIMEOUT_MS: u64 = 15_000;

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
    while started.elapsed() < std::time::Duration::from_millis(PAYLOAD_APPLY_TIMEOUT_MS) {
        let output = {
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
                Ok(snapshot) => snapshot.output.text,
                Err(error) if error.code() == "cursor_expired" => watch_state
                    .snapshot_since(None, None)
                    .map(|snapshot| snapshot.output.text)
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
        if output_has_applied_payload(&output, prompt) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    Err(TerminalDeliveryError::terminal_state_unknown(
        "payload_apply_unconfirmed",
        format!(
            "Timed out waiting for {session_id} Codex composer to apply the payload; Return was not sent"
        ),
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

fn output_has_applied_payload(output: &str, prompt: &str) -> bool {
    // The marker count can describe only a collapsed remainder. The cursor and
    // delivery lock identify the transaction, not the provider-owned count.
    if pending_paste_chars(output).is_some() {
        return true;
    }
    let token = normalize_echo_text(prompt);
    if token.is_empty() {
        return false;
    }
    let cleaned = strip_ansi_controls(output).replace('\r', "\n");
    let active_prompt = cleaned
        .rsplit_once('›')
        .map_or(cleaned.as_str(), |(_, tail)| tail);
    normalize_echo_text(active_prompt).contains(&token)
}

fn pending_paste_chars(output: &str) -> Option<usize> {
    const PREFIX: &str = "[Pasted Content ";
    const SUFFIX: &str = " chars]";

    let cleaned = strip_ansi_controls(output).replace('\r', "\n");
    let active_prompt = cleaned.rsplit_once('›')?.1;
    // Cursor movement can wrap the marker between "Pasted" and "Content".
    let active_prompt = active_prompt
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let marker_start = active_prompt.rfind(PREFIX)? + PREFIX.len();
    let remainder = &active_prompt[marker_start..];
    let marker_end = remainder.find(SUFFIX)?;
    remainder[..marker_end].trim().parse().ok()
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
        assert!(output_has_applied_payload(output, &"x".repeat(7_000)));
    }

    #[test]
    fn payload_application_requires_complete_current_payload() {
        assert!(output_has_applied_payload("\r\n› hello", "hello"));
        assert!(output_has_applied_payload(
            "\r\n› visible prefix [Pasted Content 5890 chars]",
            &"x".repeat(7_000)
        ));
        assert!(!output_has_applied_payload(
            "\r\n› first line",
            "first line\nsecond line"
        ));
        assert!(!output_has_applied_payload(
            "\r\n› hello\r\nresponse\r\n› Ask Codex to do anything",
            "hello"
        ));
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

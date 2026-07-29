//! Bounded, local presentation checkpoints for terminal scrollback recovery.
//!
//! A terminal broker owns a live PTY and remains authoritative while it is
//! present. These checkpoints exist solely for the Windows case where an app
//! restart leaves an already-running provider attached to an unrecoverable
//! ConPTY. They preserve the renderer's canonical xterm state, not raw provider
//! output, and are used only when the broker reports `SessionNotFound`.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const TERMINAL_CHECKPOINT_DIRECTORY: &str = "terminal-checkpoints";
const TERMINAL_CHECKPOINT_VERSION: u8 = 1;
pub const MAX_TERMINAL_CHECKPOINT_BYTES: usize = 1_000_000;
const MAX_TERMINAL_CHECKPOINT_COLUMNS: u16 = 1_000;
const MAX_TERMINAL_CHECKPOINT_ROWS: u16 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TerminalPresentationCheckpoint {
    pub version: u8,
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
    pub serialized_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SaveTerminalPresentationCheckpointRequest {
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
    pub serialized_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LoadTerminalPresentationCheckpointRequest {
    pub session_id: String,
}

fn validate_session_id(session_id: &str) -> Result<&str, String> {
    let trimmed = session_id.trim();
    if trimmed != session_id
        || trimmed.is_empty()
        || trimmed.len() > 128
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("Invalid terminal checkpoint session ID".to_string());
    }
    Ok(session_id)
}

fn validate_checkpoint(checkpoint: &TerminalPresentationCheckpoint) -> Result<(), String> {
    validate_session_id(&checkpoint.session_id)?;
    if checkpoint.version != TERMINAL_CHECKPOINT_VERSION {
        return Err("Unsupported terminal checkpoint version".to_string());
    }
    if checkpoint.cols == 0
        || checkpoint.rows == 0
        || checkpoint.cols > MAX_TERMINAL_CHECKPOINT_COLUMNS
        || checkpoint.rows > MAX_TERMINAL_CHECKPOINT_ROWS
    {
        return Err("Invalid terminal checkpoint geometry".to_string());
    }
    if checkpoint.serialized_state.len() > MAX_TERMINAL_CHECKPOINT_BYTES {
        return Err(format!(
            "Terminal checkpoint exceeds the {} byte limit",
            MAX_TERMINAL_CHECKPOINT_BYTES
        ));
    }
    Ok(())
}

fn checkpoint_paths(home: &Path, session_id: &str) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let session_id = validate_session_id(session_id)?;
    let directory = home.join(TERMINAL_CHECKPOINT_DIRECTORY);
    let checkpoint = directory.join(format!("{session_id}.json"));
    let previous = directory.join(format!("{session_id}.previous.json"));
    let pending = directory.join(format!("{session_id}.pending.json"));
    Ok((checkpoint, previous, pending))
}

fn save_terminal_presentation_checkpoint_for_home(
    home: &Path,
    checkpoint: TerminalPresentationCheckpoint,
) -> Result<(), String> {
    validate_checkpoint(&checkpoint)?;
    let (checkpoint_path, previous_path, pending_path) =
        checkpoint_paths(home, &checkpoint.session_id)?;
    let directory = checkpoint_path
        .parent()
        .ok_or_else(|| "Terminal checkpoint path has no parent directory".to_string())?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;

    let serialized = serde_json::to_vec(&checkpoint).map_err(|error| error.to_string())?;
    let mut pending = fs::File::create(&pending_path).map_err(|error| error.to_string())?;
    pending
        .write_all(&serialized)
        .map_err(|error| error.to_string())?;
    pending.sync_all().map_err(|error| error.to_string())?;
    drop(pending);

    // Windows does not replace an existing target with `rename`. Keep one
    // bounded previous generation so an interruption between the two moves is
    // still recoverable on the next app start.
    if previous_path.exists() {
        fs::remove_file(&previous_path).map_err(|error| error.to_string())?;
    }
    if checkpoint_path.exists() {
        fs::rename(&checkpoint_path, &previous_path).map_err(|error| error.to_string())?;
    }
    fs::rename(&pending_path, &checkpoint_path).map_err(|error| error.to_string())?;
    Ok(())
}

fn load_checkpoint_file(path: &Path, session_id: &str) -> Option<TerminalPresentationCheckpoint> {
    let contents = fs::read_to_string(path).ok()?;
    let checkpoint = serde_json::from_str::<TerminalPresentationCheckpoint>(&contents).ok()?;
    if checkpoint.session_id != session_id || validate_checkpoint(&checkpoint).is_err() {
        return None;
    }
    Some(checkpoint)
}

fn load_terminal_presentation_checkpoint_for_home(
    home: &Path,
    session_id: &str,
) -> Result<Option<TerminalPresentationCheckpoint>, String> {
    let session_id = validate_session_id(session_id)?;
    let (checkpoint_path, previous_path, _) = checkpoint_paths(home, session_id)?;
    Ok(load_checkpoint_file(&checkpoint_path, session_id)
        .or_else(|| load_checkpoint_file(&previous_path, session_id)))
}

pub(crate) fn discard_terminal_presentation_checkpoint_for_home(
    home: &Path,
    session_id: &str,
) -> Result<(), String> {
    let (checkpoint_path, previous_path, pending_path) = checkpoint_paths(home, session_id)?;
    for path in [checkpoint_path, previous_path, pending_path] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn save_terminal_presentation_checkpoint(
    request: SaveTerminalPresentationCheckpointRequest,
) -> Result<(), String> {
    let home = crate::utils::fs::get_wardian_home()
        .ok_or_else(|| "Could not resolve Wardian home".to_string())?;
    save_terminal_presentation_checkpoint_for_home(
        &home,
        TerminalPresentationCheckpoint {
            version: TERMINAL_CHECKPOINT_VERSION,
            session_id: request.session_id,
            cols: request.cols,
            rows: request.rows,
            serialized_state: request.serialized_state,
        },
    )
}

#[tauri::command]
pub async fn load_terminal_presentation_checkpoint(
    request: LoadTerminalPresentationCheckpointRequest,
) -> Result<Option<TerminalPresentationCheckpoint>, String> {
    let home = crate::utils::fs::get_wardian_home()
        .ok_or_else(|| "Could not resolve Wardian home".to_string())?;
    load_terminal_presentation_checkpoint_for_home(&home, &request.session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkpoint(session_id: &str, serialized_state: &str) -> TerminalPresentationCheckpoint {
        TerminalPresentationCheckpoint {
            version: TERMINAL_CHECKPOINT_VERSION,
            session_id: session_id.to_string(),
            cols: 120,
            rows: 36,
            serialized_state: serialized_state.to_string(),
        }
    }

    #[test]
    fn terminal_checkpoint_requests_are_strict_snake_case() {
        let request: SaveTerminalPresentationCheckpointRequest =
            serde_json::from_value(serde_json::json!({
                "session_id": "agent-123",
                "cols": 120,
                "rows": 36,
                "serialized_state": "state"
            }))
            .expect("snake case checkpoint request");
        assert_eq!(request.session_id, "agent-123");

        let camel_case = serde_json::from_value::<SaveTerminalPresentationCheckpointRequest>(
            serde_json::json!({
                "sessionId": "agent-123",
                "cols": 120,
                "rows": 36,
                "serialized_state": "state"
            }),
        );
        assert!(camel_case.is_err());
    }

    #[test]
    fn checkpoint_round_trip_is_scoped_to_a_safe_session_id() {
        let temp = tempfile::tempdir().expect("temporary Wardian home");
        let saved = checkpoint("agent-123", "\u{1b}[31mterminal history");
        save_terminal_presentation_checkpoint_for_home(temp.path(), saved.clone())
            .expect("save checkpoint");

        assert_eq!(
            load_terminal_presentation_checkpoint_for_home(temp.path(), "agent-123")
                .expect("load checkpoint"),
            Some(saved)
        );
        assert_eq!(
            load_terminal_presentation_checkpoint_for_home(temp.path(), "other-agent")
                .expect("other session load"),
            None
        );
        assert!(save_terminal_presentation_checkpoint_for_home(
            temp.path(),
            checkpoint("../outside", "state"),
        )
        .is_err());
        assert!(save_terminal_presentation_checkpoint_for_home(
            temp.path(),
            checkpoint("agent-123 ", "state"),
        )
        .is_err());
    }

    #[test]
    fn checkpoint_load_recovers_the_previous_generation_after_an_interrupted_replace() {
        let temp = tempfile::tempdir().expect("temporary Wardian home");
        let first = checkpoint("agent-123", "first state");
        let second = checkpoint("agent-123", "second state");
        save_terminal_presentation_checkpoint_for_home(temp.path(), first.clone())
            .expect("save first checkpoint");
        save_terminal_presentation_checkpoint_for_home(temp.path(), second)
            .expect("save second checkpoint");

        let (checkpoint_path, previous_path, _) =
            checkpoint_paths(temp.path(), "agent-123").expect("checkpoint paths");
        fs::write(&checkpoint_path, "incomplete").expect("corrupt current checkpoint");
        assert!(previous_path.exists());

        assert_eq!(
            load_terminal_presentation_checkpoint_for_home(temp.path(), "agent-123")
                .expect("recover previous checkpoint"),
            Some(first)
        );
    }

    #[test]
    fn checkpoint_rejects_oversized_terminal_state() {
        let temp = tempfile::tempdir().expect("temporary Wardian home");
        let oversized = "x".repeat(MAX_TERMINAL_CHECKPOINT_BYTES + 1);
        let error = save_terminal_presentation_checkpoint_for_home(
            temp.path(),
            checkpoint("agent-123", &oversized),
        )
        .expect_err("oversized checkpoint should fail");
        assert!(error.contains("byte limit"));
    }
}

//! Inspectable audit trail for privileged topology mutations.
//!
//! One JSON line per mutation attempt (allowed, denied, or a no-op), so the
//! decision the control plane made is visible on disk without reading Rust.
//! Mirrors `remote::audit`'s append+rotate shape; kept separate because the
//! two logs serve different domains (remote-gateway access vs. topology
//! authorization) and have already diverged in field shape.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const TOPOLOGY_AUDIT_SCHEMA_VERSION: u8 = 1;
const AUDIT_LOG_FILE: &str = "topology/audit.jsonl";
const AUDIT_LOG_ROTATE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopologyAuditRecord {
    pub schema_version: u8,
    pub at: String,
    /// `"operator"` or `"agent:<uuid>"`.
    pub caller: String,
    /// `"link"`, `"unlink"`, `"ignore"`, or `"unignore"`.
    pub operation: String,
    pub a: String,
    pub b: String,
    /// `"applied"`, `"unchanged"`, or `"denied"`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

pub fn audit_log_path(home: &Path) -> PathBuf {
    home.join(AUDIT_LOG_FILE)
}

fn audit_log_archive_path(home: &Path) -> PathBuf {
    home.join("topology/audit.jsonl.1")
}

pub fn append_topology_audit_record(
    home: &Path,
    record: &TopologyAuditRecord,
) -> Result<(), String> {
    append_topology_audit_record_with_limit(home, record, AUDIT_LOG_ROTATE_BYTES)
}

fn append_topology_audit_record_with_limit(
    home: &Path,
    record: &TopologyAuditRecord,
    rotate_bytes: u64,
) -> Result<(), String> {
    let path = audit_log_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    rotate_audit_log_if_needed(home, rotate_bytes)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    let line = serde_json::to_string(record).map_err(|error| error.to_string())?;
    file.write_all(line.as_bytes())
        .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())
}

fn rotate_audit_log_if_needed(home: &Path, rotate_bytes: u64) -> Result<(), String> {
    let path = audit_log_path(home);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return Ok(());
    };
    if metadata.len() < rotate_bytes {
        return Ok(());
    }
    let archive_path = audit_log_archive_path(home);
    if archive_path.exists() {
        std::fs::remove_file(&archive_path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(path, archive_path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(operation: &str, outcome: &str) -> TopologyAuditRecord {
        TopologyAuditRecord {
            schema_version: TOPOLOGY_AUDIT_SCHEMA_VERSION,
            at: "2026-08-28T00:00:00Z".to_string(),
            caller: "agent:uuid-1".to_string(),
            operation: operation.to_string(),
            a: "uuid-1".to_string(),
            b: "uuid-2".to_string(),
            outcome: outcome.to_string(),
            error_code: None,
        }
    }

    #[test]
    fn audit_append_writes_jsonl_under_topology() {
        let temp = tempfile::tempdir().expect("temp dir");
        let record = record("link", "applied");

        append_topology_audit_record(temp.path(), &record).expect("append audit");
        let log = std::fs::read_to_string(audit_log_path(temp.path())).expect("read audit log");
        let parsed: serde_json::Value = serde_json::from_str(log.trim()).expect("json audit");

        assert_eq!(parsed["schema_version"], TOPOLOGY_AUDIT_SCHEMA_VERSION);
        assert_eq!(parsed["operation"], "link");
        assert_eq!(parsed["outcome"], "applied");
        assert!(parsed.get("error_code").is_none());
    }

    #[test]
    fn audit_append_records_denied_outcome_with_error_code() {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut record = record("unlink", "denied");
        record.error_code = Some("self_serve_required".to_string());

        append_topology_audit_record(temp.path(), &record).expect("append audit");
        let log = std::fs::read_to_string(audit_log_path(temp.path())).expect("read audit log");
        let parsed: serde_json::Value = serde_json::from_str(log.trim()).expect("json audit");

        assert_eq!(parsed["outcome"], "denied");
        assert_eq!(parsed["error_code"], "self_serve_required");
    }

    #[test]
    fn audit_append_rotates_existing_log_when_limit_is_exceeded() {
        let temp = tempfile::tempdir().expect("temp dir");
        let record = record("ignore", "applied");
        let log_path = audit_log_path(temp.path());
        std::fs::create_dir_all(log_path.parent().expect("audit parent")).expect("mkdir");
        std::fs::write(&log_path, "older-entry\n".repeat(8)).expect("seed audit log");

        append_topology_audit_record_with_limit(temp.path(), &record, 16).expect("append audit");

        assert!(audit_log_archive_path(temp.path()).exists());
        let current = std::fs::read_to_string(&log_path).expect("read current audit log");
        let archived =
            std::fs::read_to_string(audit_log_archive_path(temp.path())).expect("read archive");
        assert_eq!(current.lines().count(), 1);
        assert!(current.contains("\"operation\":\"ignore\""));
        assert!(archived.contains("older-entry"));
    }
}

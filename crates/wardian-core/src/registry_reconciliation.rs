use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryQuarantineRecord {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    pub recorded_at: String,
    pub reason: String,
}

pub fn quarantine_path(home: &Path) -> PathBuf {
    home.join("settings/agent-registry-quarantine.jsonl")
}

/// Preserve evidence before a reconciliation removes a registry reference.
pub fn record_quarantine(
    home: &Path,
    session_id: &str,
    session_name: Option<&str>,
    reason: &str,
) -> io::Result<()> {
    let path = quarantine_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let record = RegistryQuarantineRecord {
        session_id: session_id.to_string(),
        session_name: session_name.map(str::to_string),
        recorded_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        reason: reason.to_string(),
    };
    let mut line = serde_json::to_string(&record).map_err(io::Error::other)?;
    line.push('\n');
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_removed_registry_evidence_as_jsonl() {
        let home = tempfile::tempdir().expect("temp home");
        record_quarantine(
            home.path(),
            "agent-lost",
            Some("Email-Triage"),
            "topology reconciliation removed a reference",
        )
        .expect("record quarantine");

        let line = std::fs::read_to_string(quarantine_path(home.path())).expect("read record");
        let record: RegistryQuarantineRecord = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(record.session_id, "agent-lost");
        assert_eq!(record.session_name.as_deref(), Some("Email-Triage"));
        assert_eq!(record.reason, "topology reconciliation removed a reference");
        assert!(!record.recorded_at.is_empty());
    }
}

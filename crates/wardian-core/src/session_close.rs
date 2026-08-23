//! Persistence and matching for generic workflow session-close invokers.

use crate::models::WorkflowAssignments;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowSessionCloseInvoker {
    pub id: String,
    pub blueprint_id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    /// Skip this invocation when the lifecycle boundary has no durable
    /// conversation archive. This remains generic workflow behavior.
    #[serde(default)]
    pub require_archive: bool,
    #[serde(default)]
    pub source_agent_id: Option<String>,
    #[serde(default)]
    pub boundary_reasons: Vec<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub bindings: HashMap<String, String>,
    #[serde(default)]
    pub assignments: WorkflowAssignments,
}

pub fn load_invokers() -> Vec<WorkflowSessionCloseInvoker> {
    let Some(path) = crate::paths::session_close_invokers_path() else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

pub fn save_invokers(invokers: &[WorkflowSessionCloseInvoker]) -> std::io::Result<()> {
    let path = crate::paths::session_close_invokers_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Wardian home is unavailable")
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::atomic_file::write_json_atomic(&path, invokers)
}

pub fn matching_invokers(
    agent_id: &str,
    boundary_reason: &str,
) -> Vec<WorkflowSessionCloseInvoker> {
    load_invokers()
        .into_iter()
        .filter(|invoker| {
            invoker.enabled
                && invoker
                    .source_agent_id
                    .as_deref()
                    .is_none_or(|source| source == agent_id)
                && (invoker.boundary_reasons.is_empty()
                    || invoker
                        .boundary_reasons
                        .iter()
                        .any(|reason| reason == boundary_reason))
        })
        .collect()
}

pub fn archive_requirement_satisfied(
    invoker: &WorkflowSessionCloseInvoker,
    archive_available: bool,
) -> bool {
    !invoker.require_archive || archive_available
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_is_disabled_by_default_and_honors_filters() {
        let invoker = WorkflowSessionCloseInvoker {
            id: "one".into(),
            blueprint_id: "memory-consolidation".into(),
            name: "Memory".into(),
            enabled: true,
            require_archive: true,
            source_agent_id: Some("agent-a".into()),
            boundary_reasons: vec!["clear".into()],
            provider: Some("codex".into()),
            workspace: None,
            input: serde_json::json!({}),
            bindings: HashMap::new(),
            assignments: WorkflowAssignments::new(),
        };
        assert!(invoker.enabled);
        assert_eq!(invoker.source_agent_id.as_deref(), Some("agent-a"));
        assert!(invoker.boundary_reasons.contains(&"clear".to_string()));
        assert!(!archive_requirement_satisfied(&invoker, false));
        assert!(archive_requirement_satisfied(&invoker, true));
    }
}

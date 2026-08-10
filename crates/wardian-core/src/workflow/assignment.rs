use std::collections::HashMap;

use crate::models::{
    AgentConversationMode, BusyPolicy, InvocationKind, WorkflowAssignments, WorkflowRoleAssignment,
};

pub fn is_known_provider(value: &str) -> bool {
    matches!(
        value,
        "claude" | "codex" | "gemini" | "antigravity" | "opencode" | "mock"
    )
}

pub fn default_busy_policy_for(invocation: InvocationKind) -> BusyPolicy {
    match invocation {
        InvocationKind::Manual => BusyPolicy::Fail,
        InvocationKind::Scheduled => BusyPolicy::Skip,
    }
}

pub fn normalize_assignments(
    assignments: Option<WorkflowAssignments>,
    legacy_bindings: &HashMap<String, String>,
    invocation: InvocationKind,
) -> WorkflowAssignments {
    let mut normalized = assignments.unwrap_or_default();
    for (role, target) in legacy_bindings {
        normalized.entry(role.clone()).or_insert_with(|| {
            if is_known_provider(target) {
                WorkflowRoleAssignment::TemporaryProvider {
                    provider: target.clone(),
                    workspace: None,
                }
            } else {
                WorkflowRoleAssignment::Agent {
                    agent_id: target.clone(),
                    conversation: AgentConversationMode::Current,
                    busy_policy: default_busy_policy_for(invocation),
                }
            }
        });
    }
    normalized
}

/// Validate the typed assignment contract shared by the UI, scheduler, and CLI.
pub fn validate_assignments(assignments: &WorkflowAssignments) -> Result<(), String> {
    for (role, assignment) in assignments {
        if role.trim().is_empty() {
            return Err("assignment role names must not be empty".to_string());
        }
        match assignment {
            WorkflowRoleAssignment::Agent { agent_id, .. } => {
                if agent_id.trim().is_empty() {
                    return Err(format!("assignment `{role}` requires a non-empty agent_id"));
                }
            }
            WorkflowRoleAssignment::TemporaryProvider {
                provider,
                workspace,
            } => {
                if !is_known_provider(provider) {
                    return Err(format!(
                        "assignment `{role}` uses unsupported provider `{provider}`"
                    ));
                }
                if workspace
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    return Err(format!(
                        "assignment `{role}` has an empty temporary-provider workspace"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Project typed assignments into the legacy role-to-target map retained for
/// older run artifacts and callers.
pub fn legacy_bindings(assignments: &WorkflowAssignments) -> HashMap<String, String> {
    assignments
        .iter()
        .map(|(role, assignment)| {
            let target = match assignment {
                WorkflowRoleAssignment::Agent { agent_id, .. } => agent_id,
                WorkflowRoleAssignment::TemporaryProvider { provider, .. } => provider,
            };
            (role.clone(), target.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn provider_legacy_binding_becomes_temporary_provider() {
        let mut bindings = HashMap::new();
        bindings.insert("summarizer".to_string(), "gemini".to_string());

        let assignments = normalize_assignments(None, &bindings, InvocationKind::Manual);

        assert_eq!(
            assignments.get("summarizer"),
            Some(&WorkflowRoleAssignment::TemporaryProvider {
                provider: "gemini".to_string(),
                workspace: None,
            })
        );
    }

    #[test]
    fn agent_legacy_binding_becomes_current_conversation_with_invocation_default() {
        let mut bindings = HashMap::new();
        bindings.insert("reasoning_gate".to_string(), "agent-123".to_string());

        let manual = normalize_assignments(None, &bindings, InvocationKind::Manual);
        let scheduled = normalize_assignments(None, &bindings, InvocationKind::Scheduled);

        assert_eq!(
            manual.get("reasoning_gate"),
            Some(&WorkflowRoleAssignment::Agent {
                agent_id: "agent-123".to_string(),
                conversation: AgentConversationMode::Current,
                busy_policy: BusyPolicy::Fail,
            })
        );
        assert_eq!(
            scheduled.get("reasoning_gate"),
            Some(&WorkflowRoleAssignment::Agent {
                agent_id: "agent-123".to_string(),
                conversation: AgentConversationMode::Current,
                busy_policy: BusyPolicy::Skip,
            })
        );
    }

    #[test]
    fn validates_typed_agent_and_temporary_provider_assignments() {
        let assignments = HashMap::from([
            (
                "planner".to_string(),
                WorkflowRoleAssignment::Agent {
                    agent_id: "agent-1".to_string(),
                    conversation: AgentConversationMode::FreshBackground,
                    busy_policy: BusyPolicy::Skip,
                },
            ),
            (
                "research".to_string(),
                WorkflowRoleAssignment::TemporaryProvider {
                    provider: "gemini".to_string(),
                    workspace: Some("<absolute-workspace-path>".to_string()),
                },
            ),
        ]);

        validate_assignments(&assignments).unwrap();
        let bindings = legacy_bindings(&assignments);
        assert_eq!(bindings.get("planner"), Some(&"agent-1".to_string()));
        assert_eq!(bindings.get("research"), Some(&"gemini".to_string()));
    }
}

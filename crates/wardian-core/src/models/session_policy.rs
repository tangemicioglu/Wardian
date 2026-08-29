use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAgentMode {
    Ephemeral,
    InheritFresh,
    InheritResume,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionPersistence {
    Fresh,
    #[default]
    Resume,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionPersistenceOverride {
    #[default]
    Default,
    Fresh,
    Resume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentExecutionPolicy {
    pub mode: AutomationAgentMode,
}

impl AgentExecutionPolicy {
    pub fn from_legacy_session_type(
        legacy_session_type: Option<&str>,
        explicit_mode: Option<&str>,
    ) -> Self {
        if let Some(mode) = explicit_mode.and_then(parse_automation_agent_mode) {
            return Self { mode };
        }

        let mode = match legacy_session_type {
            Some("temporary") => AutomationAgentMode::Ephemeral,
            Some("persistent") => AutomationAgentMode::InheritFresh,
            _ => AutomationAgentMode::Ephemeral,
        };

        Self { mode }
    }
}

pub fn parse_automation_agent_mode(value: &str) -> Option<AutomationAgentMode> {
    match value {
        "ephemeral" => Some(AutomationAgentMode::Ephemeral),
        "inherit_fresh" => Some(AutomationAgentMode::InheritFresh),
        "inherit_resume" => Some(AutomationAgentMode::InheritResume),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_temporary_maps_to_ephemeral() {
        let resolved = AgentExecutionPolicy::from_legacy_session_type(Some("temporary"), None);

        assert_eq!(resolved.mode, AutomationAgentMode::Ephemeral);
    }

    #[test]
    fn legacy_persistent_maps_to_inherit_fresh() {
        let resolved = AgentExecutionPolicy::from_legacy_session_type(Some("persistent"), None);

        assert_eq!(resolved.mode, AutomationAgentMode::InheritFresh);
    }

    #[test]
    fn explicit_mode_wins_over_legacy_session_type() {
        let resolved = AgentExecutionPolicy::from_legacy_session_type(
            Some("persistent"),
            Some("inherit_resume"),
        );

        assert_eq!(resolved.mode, AutomationAgentMode::InheritResume);
    }
}

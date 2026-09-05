use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "target_type", rename_all = "snake_case")]
pub enum AutomationRoleAssignment {
    Agent {
        agent_id: String,
        #[serde(default = "default_agent_conversation")]
        conversation: AgentConversationMode,
        #[serde(default = "default_busy_policy")]
        busy_policy: BusyPolicy,
    },
    TemporaryProvider {
        provider: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
    },
}

pub type AutomationAssignments = std::collections::HashMap<String, AutomationRoleAssignment>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentConversationMode {
    Current,
    FreshBackground,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BusyPolicy {
    Wait,
    Queue,
    Skip,
    Fail,
}

fn default_agent_conversation() -> AgentConversationMode {
    AgentConversationMode::Current
}

fn default_busy_policy() -> BusyPolicy {
    BusyPolicy::Fail
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationKind {
    Manual,
    Scheduled,
    /// Started by a listener invoker reacting to an external event.
    Listener,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
pub struct ScheduleDefinition {
    /// "interval" | "daily" | "weekly" | "monthly" | "specific_dates" | "one_time"
    pub schedule_type: String,
    /// For interval: number of minutes between runs
    #[serde(default)]
    pub interval_minutes: Option<u32>,
    /// HH:MM in local time (used by daily, weekly, monthly, specific_dates)
    #[serde(default)]
    pub time_of_day: Option<String>,
    /// For weekly: which days (e.g. ["Mon","Tue","Fri"])
    #[serde(default)]
    pub days_of_week: Option<Vec<String>>,
    /// For weekly: repeat every N weeks (default 1)
    #[serde(default = "default_repeat_every")]
    pub repeat_every: u32,
    /// For monthly: which day(s) of the month (e.g. [1, 15])
    #[serde(default)]
    pub days_of_month: Option<Vec<u32>>,
    /// For specific_dates: list of ISO date strings ["2026-05-01", "2026-06-15"]
    #[serde(default)]
    pub specific_dates: Option<Vec<String>>,
    /// ISO8601 datetime for one_time schedules
    #[serde(default)]
    pub run_at: Option<String>,
    /// End condition: "never" | "on_date" | "after_occurrences"
    #[serde(default = "default_end_condition")]
    pub end_condition: String,
    /// ISO date (YYYY-MM-DD) for end_condition = "on_date"
    #[serde(default)]
    pub end_date: Option<String>,
    /// Count for end_condition = "after_occurrences"
    #[serde(default)]
    pub max_occurrences: Option<u32>,
    /// How many times this schedule has fired (for occurrence tracking)
    #[serde(default)]
    pub occurrence_count: u32,
    pub active: bool,
}

fn default_repeat_every() -> u32 {
    1
}
fn default_end_condition() -> String {
    "never".to_string()
}

/// A persisted automation invoker: a blueprint + invocation context (input/bindings/provider)
/// that fires on a `ScheduleDefinition` cadence.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AutomationSchedule {
    pub id: String,
    /// Resolves to `<home>/library/automations/<blueprint_id>.md`.
    pub blueprint_id: String,
    pub name: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub workspace: Option<String>,
    /// Entry input params (6a `input_schema` values), passed as the run trigger.
    #[serde(default)]
    pub input: serde_json::Value,
    /// role/class -> target provider (6a bindings).
    #[serde(default)]
    pub bindings: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub assignments: AutomationAssignments,
    pub schedule: ScheduleDefinition,
    #[serde(default)]
    pub next_run_epoch_ms: Option<u64>,
    #[serde(default)]
    pub paused_remaining_ms: Option<u64>,
    #[serde(default)]
    pub is_paused: bool,
    #[serde(default)]
    pub last_run_status: Option<String>,
    #[serde(default)]
    pub last_run_error: Option<String>,
    #[serde(default)]
    pub last_run_epoch_ms: Option<u64>,
}

#[cfg(test)]
mod schedule_dto_tests {
    use super::*;

    #[test]
    fn automation_schedule_round_trips_with_defaults() {
        let json = r#"{
            "id": "s1",
            "blueprint_id": "heartbeat",
            "name": "Heartbeat",
            "schedule": { "schedule_type": "interval", "interval_minutes": 60, "active": true }
        }"#;
        let s: AutomationSchedule = serde_json::from_str(json).unwrap();
        assert_eq!(s.blueprint_id, "heartbeat");
        assert!(s.provider.is_none());
        assert!(s.input.is_null() || s.input.is_object());
        assert!(s.bindings.is_empty());
        assert!(s.assignments.is_empty());
        assert!(!s.is_paused);
        let back = serde_json::to_string(&s).unwrap();
        let s2: AutomationSchedule = serde_json::from_str(&back).unwrap();
        assert_eq!(s2.id, "s1");
    }
}

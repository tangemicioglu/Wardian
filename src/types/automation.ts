export type NodeStatus = 'idle' | 'processing' | 'completed' | 'failed' | 'blocked';

export interface AutomationTelemetryEvent {
  automation_id: string;
  node_id: string;
  status: NodeStatus;
  output?: unknown;
  error?: string;
}

export interface ScheduleDefinition {
  schedule_type: "interval" | "daily" | "weekly" | "monthly" | "specific_dates" | "one_time";
  interval_minutes?: number;
  time_of_day?: string;           // "HH:MM"
  days_of_week?: string[];        // ["Mon","Tue","Fri"]
  repeat_every?: number;          // every N weeks (default 1)
  days_of_month?: number[];       // [1, 15] for monthly
  specific_dates?: string[];      // ["2026-05-01"] for specific_dates
  run_at?: string;                // ISO datetime for one_time
  end_condition?: "never" | "on_date" | "after_occurrences";
  end_date?: string;              // YYYY-MM-DD
  max_occurrences?: number;
  occurrence_count?: number;
  active: boolean;
}

export type AutomationBusyPolicy = 'wait' | 'queue' | 'skip' | 'fail';
export type AutomationAgentConversation = 'current' | 'fresh_background';

export type AutomationRoleAssignment =
  | {
      target_type: 'agent';
      agent_id: string;
      conversation: AutomationAgentConversation;
      busy_policy?: AutomationBusyPolicy;
    }
  | {
      target_type: 'temporary_provider';
      provider: string;
      workspace?: string;
      model?: string;
      effort?: string;
    };

export type AutomationAssignments = Record<string, AutomationRoleAssignment>;

/** Mirrors the 6b Rust `AutomationSchedule` DTO (a persisted automation invoker). */
export interface AutomationSchedule {
  id: string;
  blueprint_id: string;
  name: string;
  provider?: string | null;
  workspace?: string | null;
  input: unknown;
  bindings: Record<string, string>;
  assignments?: AutomationAssignments;
  schedule: ScheduleDefinition;
  next_run_epoch_ms?: number | null;
  paused_remaining_ms?: number | null;
  is_paused: boolean;
  last_run_status?: string | null;
  last_run_error?: string | null;
  last_run_epoch_ms?: number | null;
}

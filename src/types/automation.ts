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

// ---------------------------------------------------------------------------
// Listener invokers (file watch, inbound webhook, outbound web poll)
// ---------------------------------------------------------------------------

/** What a listener does when it fires while one of its own runs is active. */
export type ListenerOverlapPolicy = 'skip' | 'coalesce' | 'parallel';

export type FileChangeKind = 'created' | 'modified' | 'removed';
export type ListenerWebhookAuth = 'token' | 'hmac_sha256';
export type ListenerPollMethod = 'get' | 'head';
export type ListenerFingerprintSource =
  | 'etag_or_last_modified'
  | 'body_hash'
  | 'json_pointer'
  | 'regex';

export interface FileWatchTrigger {
  type: 'file_watch';
  path: string;
  recursive: boolean;
  patterns: string[];
  ignore: string[];
  events: FileChangeKind[];
  debounce_ms: number;
}

export interface WebhookTrigger {
  type: 'webhook';
  path_segment: string;
  auth: ListenerWebhookAuth;
  signature_header?: string | null;
  max_body_bytes: number;
}

export interface WebPollTrigger {
  type: 'web_poll';
  url: string;
  interval_seconds: number;
  method: ListenerPollMethod;
  headers: Record<string, string>;
  fingerprint: ListenerFingerprintSource;
  json_pointer?: string | null;
  regex?: string | null;
  max_body_bytes: number;
}

export type ListenerTrigger = FileWatchTrigger | WebhookTrigger | WebPollTrigger;

export interface ListenerRejection {
  reason: string;
  at_epoch_ms: number;
}

/**
 * Every app-written field. Kept separate from the trigger config so no field
 * has two writers; the UI reads this and never sends it back.
 */
export interface ListenerRuntime {
  armed: boolean;
  arm_error?: string | null;
  last_fire_epoch_ms?: number | null;
  last_run_status?: string | null;
  last_run_error?: string | null;
  last_rejection?: ListenerRejection | null;
  fire_count: number;
  recent_fire_epoch_ms: number[];
  /** Set when the rate ceiling trips. `enabled` stays the user's own switch. */
  disabled_reason?: string | null;
  poll_fingerprint?: string | null;
  next_poll_epoch_ms?: number | null;
  consecutive_failures: number;
}

/** Mirrors the Rust `AutomationListener` DTO. */
export interface AutomationListener {
  id: string;
  blueprint_id: string;
  name: string;
  enabled: boolean;
  trigger: ListenerTrigger;
  provider?: string | null;
  workspace?: string | null;
  input: unknown;
  bindings: Record<string, string>;
  assignments?: AutomationAssignments;
  overlap?: ListenerOverlapPolicy | null;
  runtime: ListenerRuntime;
}

/** A listener plus derived facts the backend computes but never stores. */
export interface ListenerView extends AutomationListener {
  webhook_url?: string | null;
  /** Whether a secret exists. Never the secret itself. */
  has_secret: boolean;
}

export interface ListenerGatewayConfig {
  schema: number;
  host: string;
  port: number;
}

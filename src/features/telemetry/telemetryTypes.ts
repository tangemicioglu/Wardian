/**
 * DTOs for the habitat telemetry store.
 *
 * These mirror `src-tauri/src/commands/telemetry.rs` field for field. Property
 * names are `snake_case` on both sides deliberately, so nothing translates
 * across the IPC boundary and a rename in Rust surfaces here as a type error
 * rather than as a silently undefined value.
 */

/** A horizon a surface can ask about. */
export type TelemetryHorizon = "today" | "day" | "week" | "month" | "all";

/**
 * Active milliseconds, kept split by how they were established.
 *
 * There is no blended total on purpose. `measured_ms` is a duration a provider
 * reported; `clustered_ms` is inferred from gaps between events and
 * systematically runs high. Adding them produces a number with no single
 * meaning, so anything that shows one figure has to say it is a mixture.
 */
export interface ActiveTime {
  measured_ms: number;
  clustered_ms: number;
}

/**
 * Token counts, each independently nullable.
 *
 * `null` means the provider reported nothing, which is a different claim from
 * a reported zero — antigravity publishes no token accounting at all, and
 * rendering that as 0 would rank it the cheapest provider rather than the
 * unmeasured one.
 */
export interface TokenCounts {
  input_tokens: number | null;
  cached_input_tokens: number | null;
  cache_write_tokens: number | null;
  output_tokens: number | null;
  reasoning_tokens: number | null;
}

export interface TelemetrySummary {
  active: ActiveTime;
  turns: number;
  tokens: TokenCounts;
  files_touched: number;
  lines_added: number;
  lines_removed: number;
  agent_count: number;
}

export interface TelemetryBreakdownRow {
  /** The store's own key, stable across calls. */
  key: string;
  /** What to print. Differs from `key` for agents, which key on a UUID. */
  label: string;
  active: ActiveTime;
  turns: number;
  tokens: TokenCounts;
  /** Fresh input plus output; excludes cache reads. `null` when unreported. */
  billable_tokens: number | null;
  files_touched: number;
  lines_added: number;
  lines_removed: number;
  agent_count: number;
  /** False when no contributing provider reported token accounting. */
  tokens_reported: boolean;
}

export interface LimitObservation {
  provider: string;
  limit_id: string | null;
  observed_at: string;
  used_percent: number | null;
  window_minutes: number | null;
  resets_at: string | null;
  plan_type: string | null;
}

export interface HorizonWindow {
  from: string;
  to: string;
  /** True when `from` was moved back to an hour boundary to match the buckets. */
  from_floored: boolean;
}

export interface TelemetryOverview {
  window: HorizonWindow;
  summary: TelemetrySummary;
  billable_tokens: number | null;
  active_is_mixed: boolean;
  by_provider: TelemetryBreakdownRow[];
  by_agent: TelemetryBreakdownRow[];
  by_model: TelemetryBreakdownRow[];
  limits: LimitObservation[];
}

/** How an interval's duration was established. */
export type ActivityMethod = "measured" | "clustered" | "decoded";

export interface ActivityInterval {
  session_id: string;
  provider: string;
  started_at: string;
  ended_at: string;
  last_event_at: string;
  event_count: number;
  method: ActivityMethod;
}

export interface TelemetryRefreshReport {
  sources: number;
  advanced: number;
  turns: number;
  edits: number;
  intervals: number;
  buckets_recomputed: number;
  unavailable: number;
  failures: string[];
}

/** What a matrix cell counts. */
export type TelemetryMeasure =
  | "active_ms"
  | "turns"
  | "fresh_tokens"
  | "cached_tokens"
  | "output_tokens"
  | "reasoning_tokens"
  | "total_tokens"
  | "files"
  | "lines_added"
  | "lines_removed"
  | "lines_changed";

/** What the matrix rows are. */
export type TelemetryDimension = "agent" | "model" | "provider";

/** Bucket width chosen for a window. */
export type TelemetryGrain = "minute5" | "minute15" | "hour" | "six_hour" | "day";

export interface TelemetryMatrixRow {
  /** The store's own key; stable across calls and what callbacks receive. */
  key: string;
  /** What to print. Differs from `key` for agents, which key on a UUID. */
  label: string;
  /** Secondary line, e.g. an agent's class. */
  sublabel: string | null;
  /** One value per bucket, same length and order as `buckets`. */
  cells: number[];
  /**
   * The row's value over the whole window.
   *
   * For a distinct count this is deliberately NOT the sum of `cells` — a turn
   * spanning two buckets is one turn in the total and appears in both cells.
   */
  total: number;
}

export interface TelemetryMatrix {
  dimension: TelemetryDimension;
  measure: TelemetryMeasure;
  grain: TelemetryGrain;
  window: HorizonWindow;
  /** Bucket start instants, ascending and gapless. */
  buckets: string[];
  rows: TelemetryMatrixRow[];
  /** Largest single cell, for heatmap normalisation. */
  max_cell: number;
  /** True when cells do not sum to the row total (distinct-count measures). */
  cells_are_not_additive: boolean;
}

/** One agent's figures for a horizon — the Dashboard's unit. */
export interface TelemetryAgentRow {
  /** Session id. What callbacks receive; never rendered. */
  key: string;
  /** The agent's name. */
  label: string;
  /** The agent's class. */
  sublabel: string | null;
  /** Measured and inferred durations summed. */
  active_ms: number;
  turns: number;
  /** Fresh input plus output; cache reads excluded. `null` when unreported. */
  total_tokens: number | null;
  cached_tokens: number | null;
  files_touched: number;
  lines_added: number;
  lines_removed: number;
  /** False when no contributing provider reported token accounting. */
  tokens_reported: boolean;
  /** True when this agent recorded nothing in the window; still listed. */
  idle: boolean;
  /**
   * The selected measure bucketed across the window, aligned to
   * `TelemetryDashboard.buckets`.
   *
   * A total cannot tell a steady week from one frantic afternoon, which is the
   * whole reason the Dashboard carries a time axis of its own rather than
   * sending the reader to Analytics to find out.
   */
  spark: number[];
}

/**
 * One agent's rates over the Dashboard's trailing window.
 *
 * Rates rather than totals: a process viewer shows consumption now, not
 * consumption since boot. Totals ride along for the columns denominated that
 * way, and the live fields are merged in from agent state rather than the
 * telemetry store.
 */
export interface FleetRow {
  key: string;
  label: string;
  sublabel: string | null;
  /** Billable tokens per hour. `null` when the provider reports no tokens. */
  tokens_per_hour: number | null;
  turns_per_hour: number;
  active_ms: number;
  turns: number;
  total_tokens: number | null;
  files_touched: number;
  lines_added: number;
  lines_removed: number;
  tokens_reported: boolean;
  /** Nothing recorded in the window. Still listed — that is spare capacity. */
  idle: boolean;
  /** The trend measure per bucket, aligned to `TelemetryFleet.buckets`. */
  spark: number[];
  /** Live status, merged in from agent state. Absent when the agent is not loaded. */
  status?: string | null;
  cpu_usage?: number | null;
  memory_mb?: number | null;
}

/**
 * The largest value in each column across the table.
 *
 * Scaled visuals normalize against these, never against their own row: an agent
 * that ran ten minutes and one that ran all week must not draw the same shape.
 */
export interface TelemetryFleetMaxima {
  tokens_per_hour: number;
  turns_per_hour: number;
  turns: number;
  active_ms: number;
  total_tokens: number;
  files_touched: number;
  lines: number;
  spark: number;
  /** Filled in on the client, since live memory is not read from the store. */
  memory_mb?: number;
}

/**
 * One provider's contribution over the Dashboard's trailing window.
 *
 * A card in the strip above the table, not a row in it. A provider has no
 * status, no CPU, and is never "spinning", so the runaway detector the table
 * exists for has no meaning at this granularity.
 */
export interface FleetProviderRow {
  /** The provider's own name. `"all"` on the habitat card. */
  provider: string;
  /**
   * Configured agents naming this provider, whatever they did in the window.
   *
   * The ordering key, and deliberately window-independent: ordering by
   * in-window activity moves cards sideways whenever the window changes.
   */
  roster_agent_count: number;
  /** Agents on this provider that recorded anything in the window. */
  active_agent_count: number;
  active_ms: number;
  turns: number;
  /** Billable tokens. `null` when the provider publishes no token accounting. */
  total_tokens: number | null;
  files_touched: number;
  lines_added: number;
  lines_removed: number;
  tokens_reported: boolean;
  /** The trend measure per bucket, aligned to `TelemetryFleet.buckets`. */
  spark: number[];
  /** Nothing recorded in the window. Still listed, dimmed. */
  idle: boolean;
}

export interface TelemetryFleet {
  window: HorizonWindow;
  window_minutes: number;
  rows: FleetRow[];
  maxima: TelemetryFleetMaxima;
  buckets: string[];
  trend_measure: TelemetryMeasure;
  grain: TelemetryGrain;
  /**
   * The habitat as a whole, for the strip's leading card.
   *
   * Separate from `providers` rather than its first element, so nothing
   * iterating providers can pick up the total by accident.
   */
  habitat: FleetProviderRow;
  /** One card per provider, already in display order. */
  providers: FleetProviderRow[];
  /**
   * The largest value across the provider cards, excluding the habitat.
   *
   * The habitat is their sum and dominates by construction, so scaling the
   * provider cards against it would flatten every one onto the floor.
   */
  provider_maxima: TelemetryFleetMaxima;
}

/**
 * One provider's contribution to the habitat, plus its account headroom.
 *
 * Capacity is a field here rather than a component of its own. Only codex
 * publishes a limit, so a dedicated gauge made the Dashboard grow and lose a
 * whole block depending on which provider a habitat happened to run — layout
 * contingent on one vendor. As a field it is simply absent, the way every other
 * unreported measure is absent.
 */
export interface TelemetryProviderRow {
  provider: string;
  /** Agents on this provider that recorded anything in the window. */
  agent_count: number;
  active_ms: number;
  turns: number;
  total_tokens: number | null;
  files_touched: number;
  lines_added: number;
  lines_removed: number;
  /** False when this provider publishes no token accounting at all. */
  tokens_reported: boolean;
  /** Account gauges. Never per-agent, and never summed. */
  limits: LimitObservation[];
}

/** The Dashboard payload: every agent, plus account-level state. */
export interface TelemetryDashboard {
  window: HorizonWindow;
  rows: TelemetryAgentRow[];
  providers: TelemetryProviderRow[];
  /** Bucket start instants for every row's `spark`, ascending. */
  buckets: string[];
  /** Which measure the sparklines carry. */
  spark_measure: TelemetryMeasure;
  /** Bucket width, so a surface can say what one spark column covers. */
  grain: TelemetryGrain;
  /** Largest bucket across every row, so all sparklines share one scale. */
  spark_max: number;
}

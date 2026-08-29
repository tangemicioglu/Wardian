import { create } from "zustand";

import type { AgentTelemetry, AppTelemetry } from "../../types";

/**
 * The agent projections that change while nobody is interacting.
 *
 * These used to be `useState` inside `useAgentResourceController`, which runs in
 * `App`. `App` builds the entire tree in one JSX expression, so a telemetry tick
 * every five seconds — and, for `current_thoughts`, one event per line of
 * provider output — re-rendered the whole application including every mounted
 * surface and all twenty tab headers.
 *
 * Holding them here instead lets the handful of components that actually
 * display them subscribe to just the slice they read, and leaves `App` to
 * re-render only for state `App` itself owns. The roster (`agents`,
 * `off_agent_ids`) deliberately stays in the controller: `App` genuinely needs
 * it for the roster controller, git status and workspace routing, and it changes
 * rarely enough that a full render is the right trade.
 */
export type AgentTelemetryState = {
  /** Latest metrics per session, replaced only when a value actually moved. */
  telemetry: Record<string, AgentTelemetry>;
  /** Whole-application CPU and memory, from the same tick. */
  app_telemetry: AppTelemetry;
  /** Terminal-reported titles, keyed by session. */
  terminal_titles: Record<string, string>;
  /** The provider's current progress line, keyed by session. */
  current_thoughts: Record<string, string>;
};

export const EMPTY_APP_TELEMETRY: AppTelemetry = Object.freeze({
  cpu_usage: 0,
  memory_mb: 0,
});

const INITIAL_STATE: AgentTelemetryState = {
  telemetry: {},
  app_telemetry: EMPTY_APP_TELEMETRY,
  terminal_titles: {},
  current_thoughts: {},
};

export const useAgentTelemetryStore = create<AgentTelemetryState>(() => ({
  ...INITIAL_STATE,
}));

/**
 * Returns the store to its empty state.
 *
 * The controller calls this when it mounts so a remount never inherits a
 * previous session's projections, which also keeps tests independent of each
 * other without any per-test bookkeeping.
 */
export function resetAgentTelemetryStore(): void {
  useAgentTelemetryStore.setState({ ...INITIAL_STATE }, true);
}

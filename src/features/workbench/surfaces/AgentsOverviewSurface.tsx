import { memo, useEffect, useMemo, useRef } from "react";

import type {
  AgentConfig,
  AgentsOverviewMultiAgentMode,
  AgentsOverviewMode,
  AgentsOverviewSurfaceState,
  TerminalVisibility,
} from "../../../types";
import {
  AgentsOverviewView,
  type AgentsOverviewViewProps,
} from "../../../views/AgentsOverviewView";
import { keepHiddenSurfaceSnapshot } from "./hiddenSurfaceMemo";
import { useAgentTelemetryStore } from "../../agents/useAgentTelemetryStore";

type ManagedViewProps =
  | "filteredAgents"
  | "focusedAgentId"
  | "mode"
  | "onFocusedAgentChange"
  | "onExitSingle"
  | "onModeChange"
  | "surfaceId"
  // Read from the store instead: they change on every telemetry tick and on
  // every line of provider output.
  | "telemetry"
  | "terminalTitles"
  | "currentThoughts";

export interface AgentsOverviewSurfaceProps
  extends Omit<AgentsOverviewViewProps, ManagedViewProps> {
  surface_id: string;
  state: AgentsOverviewSurfaceState;
  agents: AgentConfig[];
  visibility?: TerminalVisibility;
  on_state_change: (state: AgentsOverviewSurfaceState) => void;
}

const DEFAULT_STATE: AgentsOverviewSurfaceState = {
  mode: "auto",
  last_multi_agent_mode: "auto",
  focused_agent_id: null,
  search_query: "",
  status_filter: [],
};

const PERFORMANCE_TELEMETRY_MARKER_ENABLED = import.meta.env.VITE_WARDIAN_WORKBENCH_PERF === "1";

type PerformanceTelemetryRuntime = {
  note_full_roster_telemetry_rendered?: (generation: number) => void;
};

/** Restores both the canonical state and the pre-Task-11 preview shape safely. */
export function normalizeAgentsOverviewSurfaceState(value: unknown): AgentsOverviewSurfaceState {
  if (!value || typeof value !== "object" || Array.isArray(value)) return { ...DEFAULT_STATE };
  const candidate = value as Record<string, unknown>;
  const rawMode = candidate.mode ?? candidate.presentation_mode;
  const mode = rawMode === "grid" || rawMode === "single" || rawMode === "auto"
    ? rawMode
    : DEFAULT_STATE.mode;
  const rawLastMultiAgentMode = candidate.last_multi_agent_mode;
  const last_multi_agent_mode: AgentsOverviewMultiAgentMode =
    rawLastMultiAgentMode === "auto" || rawLastMultiAgentMode === "grid"
      ? rawLastMultiAgentMode
      : mode === "grid"
        ? "grid"
        : "auto";
  return {
    mode,
    last_multi_agent_mode,
    focused_agent_id: typeof candidate.focused_agent_id === "string"
      ? candidate.focused_agent_id
      : null,
    search_query: typeof candidate.search_query === "string" ? candidate.search_query : "",
    status_filter: Array.isArray(candidate.status_filter)
      ? candidate.status_filter.filter((status): status is string => typeof status === "string")
      : [],
  };
}

function matchesSearch(agent: AgentConfig, normalizedQuery: string): boolean {
  if (!normalizedQuery) return true;
  return [
    agent.session_name,
    agent.description ?? "",
    agent.agent_class,
    agent.provider ?? "",
    agent.folder,
  ].some((value) => value.toLocaleLowerCase().includes(normalizedQuery));
}

/**
 * Focuses an agent without leaving it hidden behind surface-local filters.
 * Filters that already include the target are preserved so contextual
 * navigation does not unnecessarily discard the user's view.
 */
export function revealAgentInOverviewState(
  state: AgentsOverviewSurfaceState,
  agent: AgentConfig,
  status: string,
): AgentsOverviewSurfaceState {
  const normalizedQuery = state.search_query.trim().toLocaleLowerCase();
  const normalizedStatuses = new Set(
    state.status_filter.map((candidate) => candidate.toLocaleLowerCase()),
  );
  const matchesStatus = normalizedStatuses.size === 0
    || normalizedStatuses.has(status.toLocaleLowerCase());

  return {
    ...state,
    focused_agent_id: agent.session_id.toString(),
    search_query: matchesSearch(agent, normalizedQuery) ? state.search_query : "",
    status_filter: matchesStatus ? state.status_filter : [],
  };
}

/** Adapts persisted workbench state to the existing multi-agent view boundary. */
export const AgentsOverviewSurface = memo(function AgentsOverviewSurface({
  surface_id,
  state,
  agents,
  visibility = "visible",
  on_state_change,
  ...viewProps
}: AgentsOverviewSurfaceProps) {
  const telemetry = useAgentTelemetryStore((store) => store.telemetry);
  const terminalTitles = useAgentTelemetryStore((store) => store.terminal_titles);
  const currentThoughts = useAgentTelemetryStore((store) => store.current_thoughts);
  const stateRef = useRef(state);
  stateRef.current = state;

  const filteredAgents = useMemo(() => {
    const normalizedQuery = state.search_query.trim().toLocaleLowerCase();
    const normalizedStatuses = new Set(
      state.status_filter.map((status) => status.toLocaleLowerCase()),
    );

    return agents.filter((agent) => {
      if (!matchesSearch(agent, normalizedQuery)) return false;
      if (normalizedStatuses.size === 0) return true;
      const agentId = agent.session_id.toString();
      const metrics = telemetry[agentId];
      const title = terminalTitles[agentId] ?? "";
      const thought = currentThoughts[agentId] ?? "";
      const status = viewProps.deriveCurrentThought(
        title,
        thought,
        metrics,
        viewProps.offAgentIds.has(agentId),
      ).status.toLocaleLowerCase();
      return normalizedStatuses.has(status);
    });
  }, [
    agents,
    state.search_query,
    state.status_filter,
    currentThoughts,
    viewProps.deriveCurrentThought,
    viewProps.offAgentIds,
    telemetry,
    terminalTitles,
  ]);

  const telemetryGeneration = useMemo(() => {
    if (!PERFORMANCE_TELEMETRY_MARKER_ENABLED || agents.length === 0) return null;
    if (!agents.every((agent) => telemetry[agent.session_id] !== undefined)) return null;
    const generation = telemetry[agents[0].session_id]?.query_count;
    return Number.isSafeInteger(generation) ? generation : null;
  }, [agents, telemetry]);

  useEffect(() => {
    if (telemetryGeneration === null) return;
    const runtime = (globalThis as typeof globalThis & {
      __WARDIAN_WORKBENCH_PERF__?: PerformanceTelemetryRuntime;
    }).__WARDIAN_WORKBENCH_PERF__;
    runtime?.note_full_roster_telemetry_rendered?.(telemetryGeneration);
  }, [telemetryGeneration]);

  const updateState = (patch: Partial<AgentsOverviewSurfaceState>) => {
    const next = { ...stateRef.current, ...patch };
    stateRef.current = next;
    on_state_change(next);
  };
  const updateMode = (mode: AgentsOverviewMode) => {
    if (mode === stateRef.current.mode) return;
    if (mode === "single") {
      const previousMode = stateRef.current.mode;
      updateState({
        mode,
        last_multi_agent_mode: previousMode === "auto" || previousMode === "grid"
          ? previousMode
          : stateRef.current.last_multi_agent_mode,
      });
      return;
    }
    updateState({ mode, last_multi_agent_mode: mode });
  };
  const exitSingle = () => updateState({ mode: stateRef.current.last_multi_agent_mode });
  const updateFocusedAgent = (focused_agent_id: string | null) => {
    if (focused_agent_id === stateRef.current.focused_agent_id) return;
    updateState({ focused_agent_id });
  };

  return (
    <section
      className="flex h-full min-h-0 min-w-0 flex-col bg-[var(--color-wardian-bg)]"
      data-surface-id={surface_id}
      data-testid="agents-overview-surface"
    >
      <header className="flex flex-wrap items-center gap-2 border-b border-wardian-border bg-[var(--color-wardian-sidebar-secondary)] px-2 py-1.5">
        <div aria-label="Agents mode" className="flex items-center gap-1" role="group">
          {(["auto", "grid", "single"] as const).map((candidate) => (
            <button
              aria-pressed={state.mode === candidate}
              className="rounded px-2 py-1 text-xs text-muted-neutral transition-colors hover:text-primary aria-pressed:bg-[var(--color-wardian-accent)]/10 aria-pressed:text-[var(--color-wardian-accent)]"
              data-testid={`agents-overview-mode-${candidate}`}
              key={candidate}
              onClick={() => updateMode(candidate)}
              type="button"
            >
              {candidate[0].toUpperCase()}{candidate.slice(1)}
            </button>
          ))}
        </div>
        <label className="ml-auto flex min-w-40 items-center gap-2 text-xs text-muted-neutral">
          <span>Filter agents</span>
          <input
            aria-label="Filter Agents"
            className="min-w-0 flex-1 rounded border border-wardian-border bg-[var(--color-wardian-card)] px-2 py-1 text-primary outline-none focus:border-[var(--color-wardian-accent)]"
            onChange={(event) => updateState({ search_query: event.target.value })}
            type="search"
            value={state.search_query}
          />
        </label>
      </header>
      <div className="min-h-0 min-w-0 flex-1">
        <AgentsOverviewView
          {...viewProps}
          telemetry={telemetry}
          terminalTitles={terminalTitles}
          currentThoughts={currentThoughts}
          filteredAgents={filteredAgents}
          focusedAgentId={state.focused_agent_id}
          mode={state.mode}
          onFocusedAgentChange={updateFocusedAgent}
          onExitSingle={exitSingle}
          onModeChange={updateMode}
          surfaceId={surface_id}
          surfaceVisibility={visibility}
        />
      </div>
    </section>
  );
}, keepHiddenSurfaceSnapshot);

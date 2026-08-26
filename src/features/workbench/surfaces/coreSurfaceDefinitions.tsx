import { memo, type PropsWithChildren, type ReactNode, useEffect, useMemo, useState } from "react";

import {
  AnalyticsView,
  type AnalyticsViewProps,
} from "../../../views/AnalyticsView";
import {
  DashboardView,
  type DashboardViewProps,
} from "../../../views/DashboardView";
import { GardenView, type GardenViewProps } from "../../../views/GardenView";
import { GraphView, type GraphViewProps } from "../../../views/GraphView";
import { InboxView, type InboxViewProps } from "../../../views/InboxView";
import {
  CORE_VIEW_SURFACE_STATE_SCHEMA_VERSION,
  HEAVY_SURFACE_HIDDEN_GRACE_MS,
  type AnalyticsSurfaceState,
  type DashboardSurfaceState,
  type EmptyCoreViewSurfaceState,
  type CoreViewSurfaceType,
  type GardenSurfaceState,
  type GraphSurfaceState,
  type SurfaceVisibility,
} from "./coreSurfaceMetadata";
import { keepHiddenSurfaceSnapshot } from "./hiddenSurfaceMemo";
import { useAgentTelemetryStore } from "../../agents/useAgentTelemetryStore";

export * from "./coreSurfaceMetadata";

type SurfaceFrameProps = PropsWithChildren<{
  surface_id: string;
  surface_type: CoreViewSurfaceType;
  visibility?: SurfaceVisibility;
}>;

function SurfaceFrame({
  surface_id,
  surface_type,
  visibility = "visible",
  children,
}: SurfaceFrameProps) {
  return (
    <section
      aria-hidden={visibility === "hidden"}
      className="flex h-full min-h-0 min-w-0 flex-col bg-[var(--color-wardian-bg)]"
      data-surface-id={surface_id}
      data-surface-state-schema-version={CORE_VIEW_SURFACE_STATE_SCHEMA_VERSION}
      data-surface-type={surface_type}
      data-testid={`${surface_type}-surface`}
    >
      {children}
    </section>
  );
}

export type SuspendedSurfaceRendererProps = {
  visibility: SurfaceVisibility;
  hidden_grace_ms?: number;
  children: ReactNode | ((rendererMounted: boolean) => ReactNode);
};

/**
 * Retains the logical surface host while releasing a previously visible heavy
 * renderer after a bounded hidden grace period. A surface restored hidden does
 * not allocate the renderer until its first reveal.
 *
 * The renderer is always mounted from an effect, never during the first render.
 * Sigma and Konva cost more to build than the rest of the surface put together,
 * and building one inside the commit that reveals a tab puts that cost in the
 * frame the user is waiting on. Mounting from the effect lets the surface frame
 * and its chrome paint first; both paused states already occupy the renderer's
 * final geometry, so filling it in afterwards does not reflow the surface.
 */
export function SuspendedSurfaceRenderer({
  visibility,
  hidden_grace_ms = HEAVY_SURFACE_HIDDEN_GRACE_MS,
  children,
}: SuspendedSurfaceRendererProps) {
  const [rendererMounted, setRendererMounted] = useState(false);

  useEffect(() => {
    if (visibility === "visible") {
      setRendererMounted(true);
      return;
    }

    const timeoutId = window.setTimeout(() => {
      setRendererMounted(false);
    }, hidden_grace_ms);
    return () => window.clearTimeout(timeoutId);
  }, [hidden_grace_ms, visibility]);

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-col"
      data-heavy-renderer-state={rendererMounted ? "mounted" : "released"}
    >
      {typeof children === "function"
        ? children(rendererMounted)
        : rendererMounted ? children : null}
    </div>
  );
}

type ManagedSurfaceProps<TState> = {
  surface_id: string;
  state: TState;
  visibility?: SurfaceVisibility;
};

export interface DashboardSurfaceProps
  extends Omit<DashboardViewProps, "live">, ManagedSurfaceProps<DashboardSurfaceState> {
  /** The roster to join against live metrics; the join happens here, not in `App`. */
  live_agents: readonly { session_id: string }[];
}

export function DashboardSurface({
  surface_id,
  // The Dashboard is a singleton surface, so its configuration is held by the
  // app and mirrored here. The state contract exists so that the day it becomes
  // multi-instance, each one already persists its own columns and window.
  state: _state,
  visibility = "visible",
  live_agents,
  ...viewProps
}: DashboardSurfaceProps) {
  // The instant columns read live agent state; every rate comes from the
  // telemetry store. Joining them here rather than in `App` keeps a five-second
  // tick off the whole-application render path.
  const telemetry = useAgentTelemetryStore((state) => state.telemetry);
  const live = useMemo(() => live_agents.map((agent) => ({
    session_id: agent.session_id,
    status: telemetry[agent.session_id]?.current_status ?? null,
    cpu_usage: telemetry[agent.session_id]?.cpu_usage ?? null,
    memory_mb: telemetry[agent.session_id]?.memory_mb ?? null,
  })), [live_agents, telemetry]);
  return (
    <SurfaceFrame surface_id={surface_id} surface_type="dashboard" visibility={visibility}>
      <DashboardView {...viewProps} live={live} />
    </SurfaceFrame>
  );
}

export interface AnalyticsSurfaceProps
  extends Omit<AnalyticsViewProps, "initial">, ManagedSurfaceProps<AnalyticsSurfaceState> {}

export function AnalyticsSurface({
  surface_id,
  state,
  visibility = "visible",
  ...viewProps
}: AnalyticsSurfaceProps) {
  return (
    <SurfaceFrame surface_id={surface_id} surface_type="analytics" visibility={visibility}>
      {/* The surface state is the grid's opening question, which is how a
          Dashboard drill-through arrives already scoped to one agent. */}
      <AnalyticsView {...viewProps} initial={state} />
    </SurfaceFrame>
  );
}

export interface InboxSurfaceProps
  extends InboxViewProps, ManagedSurfaceProps<EmptyCoreViewSurfaceState> {}

export function InboxSurface({
  surface_id,
  state: _state,
  visibility = "visible",
  ...viewProps
}: InboxSurfaceProps) {
  return (
    <SurfaceFrame surface_id={surface_id} surface_type="inbox" visibility={visibility}>
      <InboxView {...viewProps} />
    </SurfaceFrame>
  );
}

export interface GraphSurfaceProps
  extends Omit<GraphViewProps,
    "onOpenAgentInGrid" | "visibility" | "rendererActive" | "initialSurfaceState"
    | "onSurfaceStateChange" | "telemetry" | "terminalTitles" | "currentThoughts">,
    ManagedSurfaceProps<GraphSurfaceState> {
  onOpenAgent: (agentId: string) => void;
  on_state_change: (state: GraphSurfaceState) => void;
}

export const GraphSurface = memo(function GraphSurface({
  surface_id,
  state: _state,
  on_state_change,
  visibility = "visible",
  ...viewProps
}: GraphSurfaceProps) {
  // Read straight from the store. These change on every telemetry tick and,
  // for thoughts, on every line of provider output; routing them through
  // `App` made each of those a whole-application render.
  const telemetry = useAgentTelemetryStore((state) => state.telemetry);
  const terminalTitles = useAgentTelemetryStore((state) => state.terminal_titles);
  const currentThoughts = useAgentTelemetryStore((state) => state.current_thoughts);
  return (
    <SurfaceFrame surface_id={surface_id} surface_type="graph" visibility={visibility}>
      <SuspendedSurfaceRenderer visibility={visibility}>
        {(rendererMounted) => (
          <GraphView
            {...viewProps}
            telemetry={telemetry}
            terminalTitles={terminalTitles}
            currentThoughts={currentThoughts}
            visibility={visibility}
            rendererActive={rendererMounted}
            initialSurfaceState={_state}
            onSurfaceStateChange={(state) => {
              if (JSON.stringify(state) !== JSON.stringify(_state)) on_state_change(state);
            }}
          />
        )}
      </SuspendedSurfaceRenderer>
    </SurfaceFrame>
  );
}, keepHiddenSurfaceSnapshot);

export interface GardenSurfaceProps
  extends Omit<GardenViewProps,
    "onOpenAgentInGrid" | "visibility" | "rendererActive" | "initialSurfaceState"
    | "onSurfaceStateChange" | "telemetry">,
    ManagedSurfaceProps<GardenSurfaceState> {
  onOpenAgent: (agentId: string) => void;
  on_state_change: (state: GardenSurfaceState) => void;
}

export const GardenSurface = memo(function GardenSurface({
  surface_id,
  state: _state,
  on_state_change,
  visibility = "visible",
  ...viewProps
}: GardenSurfaceProps) {
  // Read straight from the store. These change on every telemetry tick and,
  // for thoughts, on every line of provider output; routing them through
  // `App` made each of those a whole-application render.
  const telemetry = useAgentTelemetryStore((state) => state.telemetry);
  return (
    <SurfaceFrame surface_id={surface_id} surface_type="garden" visibility={visibility}>
      <SuspendedSurfaceRenderer visibility={visibility}>
        {(rendererMounted) => (
          <GardenView
            {...viewProps}
            telemetry={telemetry}
            visibility={visibility}
            rendererActive={rendererMounted}
            initialSurfaceState={_state}
            onSurfaceStateChange={(state) => {
              if (JSON.stringify(state) !== JSON.stringify(_state)) on_state_change(state);
            }}
          />
        )}
      </SuspendedSurfaceRenderer>
    </SurfaceFrame>
  );
}, keepHiddenSurfaceSnapshot);

import React, { useEffect, useMemo, useRef, useState } from "react";
import type { AgentConfig, AgentTelemetry } from "../types";
import type { AgentInteractions, AgentTeam, Watchlist } from "../layout/watchlist/types";
import { buildAgentGraph, type GraphRelationshipReason } from "../features/graph/graphProjection";
import { buildGardenUnits } from "../features/garden/gardenProjection";
import { GardenCanvas } from "../features/garden/GardenCanvas";
import { unitKey } from "../features/garden/garden.types";
import { GARDEN_AGENT_STATUS_LEGEND, gardenAgentStatusLabel, gardenWorkflowStatusLabel } from "../features/garden/gardenStatus";
import { useGardenWorkflows } from "../features/garden/useGardenWorkflows";
import { scenesConverged } from "../features/garden/gardenScene";
import { useGardenStore } from "../store/useGardenStore";
import type { GardenSurfaceState } from "../features/workbench/surfaces/coreSurfaceMetadata";

const ALL_REASONS: Set<GraphRelationshipReason> = new Set([
  "same_team",
  "shared_workspace",
  "same_worktree",
]);

export interface GardenViewProps {
  visibility?: "visible" | "hidden";
  rendererActive?: boolean;
  initialSurfaceState?: GardenSurfaceState;
  onSurfaceStateChange?: (state: GardenSurfaceState) => void;
  filteredAgents: AgentConfig[];
  telemetry: Record<string, AgentTelemetry>;
  teams: AgentTeam[];
  activeList: Watchlist | null;
  interactions: AgentInteractions;
  selectedAgentIds: Set<string>;
  offAgentIds: Set<string>;
  onSelectionChange: (ids: Set<string>) => void;
  onOpenAgent?: (agentId: string) => void;
  /** @deprecated Legacy flag-off adapter. Workbench surfaces use onOpenAgent. */
  onOpenAgentInGrid?: (agentId: string) => void;
}

export const GardenView: React.FC<GardenViewProps> = ({
  filteredAgents,
  telemetry,
  teams,
  activeList,
  interactions,
  selectedAgentIds,
  offAgentIds,
  onSelectionChange,
  onOpenAgent,
  onOpenAgentInGrid,
  visibility = "visible",
  rendererActive = true,
  initialSurfaceState,
  onSurfaceStateChange,
}) => {
  const scene = useGardenStore((s) => s.scene);
  const pinUnit = useGardenStore((s) => s.pin);
  const visitUnit = useGardenStore((s) => s.visit);
  const adoptScene = useGardenStore((s) => s.adoptScene);
  const resetLayout = useGardenStore((s) => s.reset);
  const workflowInputs = useGardenWorkflows(visibility === "visible");

  // Canvas highlight is keyed by unitKey so agent and workflow ids can't collide,
  // and it stays local so selecting a workflow never leaks into the app's
  // agent-only selection set. Agent clicks still propagate up (for Grid routing).
  const [selectedKey, setSelectedKey] = useState<string | null>(
    initialSurfaceState?.selected_unit_key ?? null,
  );
  const onSurfaceStateChangeRef = useRef(onSurfaceStateChange);
  onSurfaceStateChangeRef.current = onSurfaceStateChange;
  useEffect(() => {
    onSurfaceStateChangeRef.current?.({ selected_unit_key: selectedKey });
  }, [selectedKey]);

  const projection = useMemo(
    () =>
      buildAgentGraph({
        agents: filteredAgents,
        telemetry,
        teams,
        activeList,
        interactions,
        selectedAgentIds,
        enabledReasons: ALL_REASONS,
        offAgentIds,
      }),
    [filteredAgents, telemetry, teams, activeList, interactions, selectedAgentIds, offAgentIds],
  );

  // Geometry is derived from the metric pipeline. Telemetry deliberately does
  // not reach it — status and colour ride along on the projection as display
  // channels only, so a status change repaints without moving anything.
  const layout = useMemo(
    () => buildGardenUnits({ projection, teams, workflows: workflowInputs, scene }),
    [projection, teams, workflowInputs, scene],
  );
  const { agentUnits, workflowUnits, placement } = layout;

  // Persist district cells and settled positions so the next pass can warm-start
  // from them; without that the map re-derives from scratch on every reload and
  // visibly rearranges itself.
  //
  // The convergence check is load-bearing rather than an optimisation: a layout
  // pass always returns a fresh scene object, so writing back on identity would
  // re-trigger the layout forever. `scenesConverged` stops once positions settle
  // within a fraction of a pixel.
  const adoptSceneRef = useRef(adoptScene);
  adoptSceneRef.current = adoptScene;
  useEffect(() => {
    if (!scenesConverged(scene, layout.scene)) adoptSceneRef.current(layout.scene);
  }, [layout.scene, scene]);

  // Fall back to an externally-selected single agent (e.g. chosen in Grid) when
  // there is no local Garden selection yet.
  const externalAgentKey =
    selectedAgentIds.size === 1 ? unitKey({ kind: "agent", id: [...selectedAgentIds][0] }) : null;
  const activeSelectionKey = selectedKey ?? externalAgentKey;
  const selectedUnit = [...agentUnits, ...workflowUnits].find((unit) => unitKey(unit.ref) === activeSelectionKey);
  const selectedUnitStatus = selectedUnit
    ? "status" in selectedUnit
      ? gardenAgentStatusLabel(selectedUnit.status)
      : gardenWorkflowStatusLabel(selectedUnit.runStatus)
    : null;

  return (
    <div className="garden-view relative flex min-h-0 flex-1 flex-col">
      <section
        aria-label="Garden status legend"
        className="absolute left-3 top-3 z-10 flex max-w-[calc(100%-1.5rem)] flex-wrap items-center gap-x-3 gap-y-1 rounded-md border border-wardian-border bg-[var(--color-wardian-bg)]/90 px-2 py-1.5 text-[10px] shadow-sm backdrop-blur"
      >
        <span className="font-bold text-primary">Status</span>
        {GARDEN_AGENT_STATUS_LEGEND.map((item) => (
          <span key={item.label} className="inline-flex items-center gap-1 text-muted-neutral">
            <span className={`h-1.5 w-1.5 rounded-full ${item.indicatorClass}`} aria-hidden="true" />
            {item.label}
          </span>
        ))}
      </section>
      <div
        id="garden-selection-summary"
        data-testid="garden-selection-summary"
        aria-live="polite"
        className="absolute bottom-3 left-3 z-10 rounded-md border border-wardian-border bg-[var(--color-wardian-bg)]/90 px-2 py-1.5 text-[11px] shadow-sm backdrop-blur"
      >
        {selectedUnit && selectedUnitStatus ? (
          <><span className="text-muted">Selected: </span><span className="font-semibold text-primary">{selectedUnit.label}</span><span className="text-muted"> · {selectedUnitStatus}</span></>
        ) : (
          <span className="text-muted">Select a unit to view its status.</span>
        )}
      </div>
      {rendererActive ? <GardenCanvas
        agentUnits={agentUnits}
        workflowUnits={workflowUnits}
        selectedKey={activeSelectionKey}
        onSelect={(ref) => {
          const key = unitKey(ref);
          setSelectedKey(key);
          visitUnit(key);
          if (ref.kind === "agent") {
            onSelectionChange(new Set([ref.id]));
          }
        }}
        onOpenAgent={(agentId) => (onOpenAgent ?? onOpenAgentInGrid)?.(agentId)}
        onMoveUnit={(key, x, y) => {
          // A drag is a pin, stored relative to the unit's district so the
          // placement survives the district being relocated on the grid.
          const where = placement.get(key);
          if (!where) return;
          pinUnit(key, where.districtId, { x, y }, where.districtOrigin);
        }}
        onResetLayout={() => {
          resetLayout();
          setSelectedKey(null);
        }}
      /> : (
        <div className="flex flex-1 items-center justify-center text-sm text-muted">
          Garden renderer paused while hidden
        </div>
      )}
    </div>
  );
};

import React, { useEffect, useMemo, useRef, useState } from "react";
import type { AgentConfig, AgentTelemetry } from "../types";
import type { AgentInteractions, AgentTeam, Watchlist } from "../layout/watchlist/types";
import { buildAgentGraph, type GraphRelationshipReason } from "../features/graph/graphProjection";
import {
  buildAgentUnits,
  buildLibraryUnits,
  buildWorkflowUnits,
  computeGardenLayout,
  gardenLayoutSignature,
} from "../features/garden/gardenProjection";
import { GardenCanvas } from "../features/garden/GardenCanvas";
import { unitKey } from "../features/garden/garden.types";
import {
  GARDEN_AGENT_STATUS_LEGEND,
  gardenAgentStatusLabel,
  gardenLibraryDeploymentLabel,
  gardenWorkflowStatusLabel,
} from "../features/garden/gardenStatus";
import { useGardenWorkflows } from "../features/garden/useGardenWorkflows";
import { useGardenLibrary } from "../features/garden/useGardenLibrary";
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
  const libraryInputs = useGardenLibrary(visibility === "visible");

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

  // Layout output — district cells and settled positions — is carried forward
  // through a ref, never through the reactive dependency chain.
  //
  // Routing it back through state makes the layout an input to itself: each pass
  // produces slightly different positions, which triggers another pass. A
  // convergence epsilon is not a sufficient brake, because the pipeline is not
  // guaranteed to converge monotonically — overlap removal ranks units by their
  // incoming positions, so two units sitting at nearly the same coordinate can
  // swap separation order between passes and oscillate indefinitely. When that
  // happens the write-back becomes an unbounded render loop (React error #185)
  // rather than a slightly jittery map.
  //
  // Excluding positions from the dependencies also states the intent correctly:
  // settled positions are a *result*, and only genuine inputs — the roster, the
  // teams, the blueprints, and the user's own placements — should provoke a
  // relayout.
  const carriedSceneRef = useRef(scene);

  // Geometry is keyed on a digest of the inputs that may legitimately move
  // something, not on the projection's identity. The projection is rebuilt on
  // every telemetry tick; running the pipeline that often would advance
  // convergence a few pixels each time and the map would drift continuously for
  // no reason a user could see. `projectionRef` supplies the current value
  // without making identity a trigger.
  const { pins, exclusions } = scene;
  const projectionRef = useRef(projection);
  projectionRef.current = projection;
  const signature = useMemo(
    () => gardenLayoutSignature(projection, teams, workflowInputs, libraryInputs),
    [projection, teams, workflowInputs, libraryInputs],
  );

  const layout = useMemo(() => {
    const result = computeGardenLayout({
      projection: projectionRef.current,
      teams,
      workflows: workflowInputs,
      library: libraryInputs,
      scene: { ...carriedSceneRef.current, pins, exclusions },
    });
    carriedSceneRef.current = result.scene;
    return result;
    // `signature` stands in for projection/teams/workflows: it captures exactly
    // the geometry-relevant content and omits status, colour, and selection.
  }, [signature, pins, exclusions]);
  const { placement } = layout;

  // Display fields are attached per render from the live projection, so status
  // and colour stay current without touching geometry.
  const agentUnits = useMemo(
    () => buildAgentUnits(projection, layout.positions),
    [projection, layout.positions],
  );
  const workflowUnits = useMemo(
    () => buildWorkflowUnits(workflowInputs, layout.positions),
    [workflowInputs, layout.positions],
  );
  const libraryUnits = useMemo(
    () => buildLibraryUnits(libraryInputs, layout.positions),
    [libraryInputs, layout.positions],
  );

  // Persist district cells and settled positions so a later session can
  // warm-start from them; without that the map re-derives from scratch on every
  // reload and visibly rearranges itself. `adoptScene` ignores a scene that has
  // not moved materially, so this cannot churn storage.
  const adoptSceneRef = useRef(adoptScene);
  adoptSceneRef.current = adoptScene;
  useEffect(() => {
    adoptSceneRef.current(layout.scene);
  }, [layout]);

  // Fall back to an externally-selected single agent (e.g. chosen in Grid) when
  // there is no local Garden selection yet.
  const externalAgentKey =
    selectedAgentIds.size === 1 ? unitKey({ kind: "agent", id: [...selectedAgentIds][0] }) : null;
  const activeSelectionKey = selectedKey ?? externalAgentKey;
  const selectedUnit = [...agentUnits, ...workflowUnits, ...libraryUnits].find(
    (unit) => unitKey(unit.ref) === activeSelectionKey,
  );
  const selectedUnitStatus = selectedUnit
    ? "status" in selectedUnit
      ? gardenAgentStatusLabel(selectedUnit.status)
      : "runStatus" in selectedUnit
        ? gardenWorkflowStatusLabel(selectedUnit.runStatus)
        : gardenLibraryDeploymentLabel(selectedUnit.deploymentCount)
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
        libraryUnits={libraryUnits}
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

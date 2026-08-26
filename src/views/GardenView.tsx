import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AgentConfig, AgentTelemetry } from "../types";
import type { AgentInteractions, AgentTeam, Watchlist } from "../layout/watchlist/types";
import { buildAgentGraph, type GraphRelationshipReason } from "../features/graph/graphProjection";
import {
  buildAgentUnits,
  buildWorkflowUnits,
  computeGardenLayout,
  gardenLayoutSignature,
} from "../features/garden/gardenProjection";
import { GardenCanvas } from "../features/garden/GardenCanvas";
import { unitKey, type GardenPosition } from "../features/garden/garden.types";
import {
  GARDEN_AGENT_STATUS_LEGEND,
  GARDEN_AREA_NOTE,
  GARDEN_CENTRALITY_NOTE,
  GARDEN_CHANGE_LEGEND,
  gardenAgentStatusLabel,
  gardenChangeBaselineLabel,
  gardenGroundLabel,
  gardenSkillReachLabel,
  gardenWorkflowStatusLabel,
} from "../features/garden/gardenStatus";
import { agentsCarrying } from "../features/garden/skillGlyphs";
import { useGardenWorkflows } from "../features/garden/useGardenWorkflows";
import { useGardenSkills } from "../features/garden/useGardenSkills";
import { useGardenReach } from "../features/garden/useGardenReach";
import { useGardenTerrain } from "../features/garden/useGardenTerrain";
import { useTerrainChanges } from "../features/garden/useTerrainChanges";
import { useTerrainOpen } from "../features/garden/useTerrainOpen";
import { basename as terrainCellName } from "../features/garden/terrain";
import type { TerrainViewport } from "../features/garden/terrainFrontier";
import { useGardenStore } from "../store/useGardenStore";
import { useLibraryStore } from "../store/useLibraryStore";
import type { GardenSurfaceState } from "../features/workbench/surfaces/coreSurfaceMetadata";

const ALL_REASONS: Set<GraphRelationshipReason> = new Set([
  "same_team",
  "shared_workspace",
  "same_worktree",
]);

/** Shared empty set, so "nothing highlighted" keeps a stable identity. */
const EMPTY_HIGHLIGHT: ReadonlySet<string> = new Set<string>();

/**
 * Hold a dropped unit inside the district it belongs to.
 *
 * A drag says where within its neighbourhood a unit should sit. It cannot say
 * which neighbourhood the unit is in — that comes from canonical facts about the
 * agent, not from where a cursor was released — so a drop into a neighbouring
 * district is a placement the map cannot honour without lying about membership.
 *
 * Letting it through was worse than an approximation. The unit stayed a member
 * of its own district at an enormous offset, so the district grew to that size,
 * every ring grew with it, and the unit rode its own origin outward: dropped on
 * a neighbour, it landed 600 units away, and the map inflated 2.3x in one drag.
 *
 * Clamping to the boundary keeps the gesture honest — you can place a unit
 * anywhere in its own territory, and the edge is where the territory ends.
 */
function clampToDistrict(
  point: GardenPosition,
  where: { districtOrigin: GardenPosition; districtRadius: number },
): GardenPosition {
  const dx = point.x - where.districtOrigin.x;
  const dy = point.y - where.districtOrigin.y;
  const distance = Math.hypot(dx, dy);
  if (distance <= where.districtRadius || distance === 0) return point;
  const scale = where.districtRadius / distance;
  return {
    x: where.districtOrigin.x + dx * scale,
    y: where.districtOrigin.y + dy * scale,
  };
}

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
  const {
    workflows: workflowInputs,
    truncated: workflowsTruncated,
    nextOffset: workflowsNextOffset,
    loadMore: loadMoreWorkflows,
  } = useGardenWorkflows(visibility === "visible");
  const skillInputs = useGardenSkills(visibility === "visible");
  // Deep-links into the Library the same way the agent config panel's "Manage
  // skills" affordance does, so the Garden does not invent a second navigation
  // path to the same surface.
  const openLibraryAt = useLibraryStore((s) => s.openLibraryAt);

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

  // Which districts coordinate others, so the lattice can seat them nearer the
  // middle. Roots come from the roster rather than from `layout.districts`,
  // because this feeds the layout and reading them back out of it would close a
  // loop. Fetched once per root set — see `useGardenReach` on why geometry does
  // not subscribe to writes the way the paint does.
  const reach = useGardenReach(visibility === "visible", filteredAgents);

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
  // Carried the same way and for the same reason: `signature` already covers
  // every reach change that may move something, so the array's identity must
  // not be a second trigger.
  const reachRef = useRef(reach);
  reachRef.current = reach;
  const signature = useMemo(
    () => gardenLayoutSignature(projection, teams, workflowInputs, skillInputs, reach),
    [projection, teams, workflowInputs, skillInputs, reach],
  );

  // A reset discards the scene rather than advancing it, and the carried copy
  // would otherwise put everything straight back: it holds the settled positions
  // and district cells the reset just cleared, and the next pass warm-starts
  // from them. The counter is what tells the two cases apart.
  const generation = useGardenStore((s) => s.generation);
  const generationRef = useRef(generation);

  const layout = useMemo(() => {
    if (generationRef.current !== generation) {
      generationRef.current = generation;
      carriedSceneRef.current = scene;
    }
    const result = computeGardenLayout({
      projection: projectionRef.current,
      teams,
      workflows: workflowInputs,
      skills: skillInputs,
      reach: reachRef.current,
      scene: { ...carriedSceneRef.current, pins, exclusions },
    });
    carriedSceneRef.current = result.scene;
    return result;
    // `signature` stands in for projection/teams/workflows: it captures exactly
    // the geometry-relevant content and omits status, colour, and selection.
    //
    // `scene` is read only on a generation change, and is deliberately not a
    // dependency: it is republished by every adopted layout, and depending on it
    // would make the layout an input to itself.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, pins, exclusions, generation]);
  const { placement } = layout;
  // How many districts the arrangement is actually making a claim about. Zero
  // means every district keeps to itself and the centrality legend would be
  // explaining a distinction nothing on screen is drawing.
  const coordinatingDistricts = useMemo(
    () => [...layout.reachTiers.values()].filter((tier) => tier > 0).length,
    [layout.reachTiers],
  );

  // Display fields are attached per render from the live projection, so status
  // and colour stay current without touching geometry.
  const agentUnits = useMemo(
    () => buildAgentUnits(projection, layout.positions, layout.crowns),
    [projection, layout.positions, layout.crowns],
  );
  const workflowUnits = useMemo(
    () => buildWorkflowUnits(workflowInputs, layout.positions),
    [workflowInputs, layout.positions],
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

  // A selected skill has no unit of its own, so it is summarized from its
  // carriers instead — and those carriers light up on the map. This is the
  // reverse index that replaces "go to where the skill lives": the honest
  // answer to "where is this used?" is a set, not a point.
  const selectedSkillRef = activeSelectionKey?.startsWith("skill:")
    ? activeSelectionKey.slice("skill:".length)
    : null;
  const selectedSkillLabel = selectedSkillRef
    ? (skillInputs.find((skill) => skill.entryRef === selectedSkillRef)?.label ?? selectedSkillRef)
    : null;
  const selectedPath = activeSelectionKey?.startsWith("path:")
    ? activeSelectionKey.slice("path:".length)
    : null;
  const selectedAgentId = activeSelectionKey?.startsWith("agent:")
    ? activeSelectionKey.slice("agent:".length)
    : null;

  // Terrain is ingested against the visible world rectangle, so the canvas
  // reports its viewport up. It arrives already coalesced to one value per
  // animation frame; the expansion pass inside the hook debounces on top of
  // that, so a pan produces listings once it settles rather than while it moves.
  const [viewport, setViewport] = useState<TerrainViewport | null>(null);
  const terrainEnabled = visibility === "visible" && rendererActive;
  const terrain = useGardenTerrain({
    enabled: terrainEnabled,
    districts: layout.districts,
    viewport,
  });
  // Change review is fetched per visible workspace root, not per agent:
  // attribution already spans every conversation in a workspace, so one call
  // answers for all of them.
  const changes = useTerrainChanges({ enabled: terrainEnabled, roots: terrain.visibleRoots });

  // Both reverse indexes, in one place. A skill and a piece of ground answer
  // "who?" the same way — with a set of agents — because neither is a thing you
  // can navigate to. Selecting an agent runs it the other direction and lights
  // up the disk it has written to, across every district.
  const highlightedAgentIds = useMemo(() => {
    if (selectedSkillRef) return agentsCarrying(layout.crowns, selectedSkillRef);
    if (selectedPath) return new Set(changes.paint.get(selectedPath)?.agentIds ?? []);
    return EMPTY_HIGHLIGHT;
  }, [selectedSkillRef, selectedPath, layout.crowns, changes.paint]);

  const highlightedPaths = useMemo(() => {
    if (!selectedAgentId) return EMPTY_HIGHLIGHT;
    const written = new Set<string>();
    for (const [path, paint] of changes.paint) {
      if (paint.agentIds.includes(selectedAgentId)) written.add(path);
    }
    return written;
  }, [selectedAgentId, changes.paint]);

  const openPath = useTerrainOpen({ entries: changes.entries, baseline: changes.baseline });
  // Stable identities: `TerrainLayer` is memoized, and a fresh empty set or a
  // fresh closure per render would re-render two thousand ground cells on every
  // telemetry tick.
  const handleSelectPath = useCallback(
    (path: string) => setSelectedKey(unitKey({ kind: "path", id: path })),
    [],
  );

  const selectedUnit = [...agentUnits, ...workflowUnits].find(
    (unit) => unitKey(unit.ref) === activeSelectionKey,
  );
  const selectedPaint = selectedPath ? changes.paint.get(selectedPath) : undefined;
  const summaryLabel =
    selectedSkillLabel ?? (selectedPath ? terrainCellName(selectedPath) : null) ?? selectedUnit?.label ?? null;
  const summaryStatus = selectedSkillRef
    ? gardenSkillReachLabel(highlightedAgentIds.size)
    : selectedPath
      ? gardenGroundLabel(selectedPaint)
      : selectedUnit
        ? "status" in selectedUnit
          ? gardenAgentStatusLabel(selectedUnit.status)
          : gardenWorkflowStatusLabel(selectedUnit.runStatus)
        : null;

  return (
    <div className="garden-view relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
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
        {changes.paint.size > 0 && (
          <>
            <span className="h-3 w-px bg-wardian-border" aria-hidden="true" />
            {/* The baseline is named rather than assumed: the ground uses a
                workspace-level baseline while the Changes pane may be showing an
                agent-scoped one, and the two must not differ silently. */}
            <span className="font-bold text-primary" title={gardenChangeBaselineLabel(changes.baseline)}>
              Ground
            </span>
            {GARDEN_CHANGE_LEGEND.map((item) => (
              <span key={item.kind} className="inline-flex items-center gap-1 text-muted-neutral">
                <span
                  className="h-1.5 w-1.5 rounded-sm"
                  style={{ backgroundColor: item.colorVar }}
                  aria-hidden="true"
                />
                {item.label}
              </span>
            ))}
            <span className="text-muted-neutral">{gardenChangeBaselineLabel(changes.baseline)}</span>
            {/* Area is a share of the parent folder, not a file size, and it
                reads as size unless it is said out loud — a loose file at a
                repository root is a peer of `src/` and is drawn as one. */}
            <span className="text-muted-neutral" title={GARDEN_AREA_NOTE}>
              Area = share of folder
            </span>
          </>
        )}
        {coordinatingDistricts > 0 && (
          <>
            <span className="h-3 w-px bg-wardian-border" aria-hidden="true" />
            {/* An arrangement that encodes a claim nobody can read is no better
                than one that encodes nothing, so the map says what its middle
                means — and only when some district is actually claiming it. */}
            <span className="text-muted-neutral" title={GARDEN_CENTRALITY_NOTE}>
              Centre = coordinates others
            </span>
          </>
        )}
      </section>
      <div
        id="garden-selection-summary"
        data-testid="garden-selection-summary"
        aria-live="polite"
        className="absolute bottom-3 left-3 z-10 rounded-md border border-wardian-border bg-[var(--color-wardian-bg)]/90 px-2 py-1.5 text-[11px] shadow-sm backdrop-blur"
      >
        {summaryLabel && summaryStatus ? (
          <><span className="text-muted">Selected: </span><span className="font-semibold text-primary">{summaryLabel}</span><span className="text-muted"> · {summaryStatus}</span></>
        ) : (
          <span className="text-muted">Select a unit to view its status.</span>
        )}
      </div>
      {terrain.truncatedRoots.size > 0 && (
        <div className="absolute bottom-3 right-3 z-10 rounded-md border border-[var(--color-wardian-warning)]/40 bg-[var(--color-wardian-warning)]/10 px-2 py-1.5 text-[11px] text-[var(--color-wardian-warning)] shadow-sm" role="status">
          <span>Folder contents load 500 items at a time.</span>{' '}
          <button
            type="button"
            className="font-semibold underline disabled:opacity-50"
            onClick={() => void Promise.all([...terrain.truncatedRoots].map((path) => terrain.loadMoreRoot(path)))}
          >
            Load next page
          </button>
        </div>
      )}
      {workflowsTruncated && (
        <div className="absolute bottom-14 right-3 z-10 rounded-md border border-[var(--color-wardian-warning)]/40 bg-[var(--color-wardian-warning)]/10 px-2 py-1.5 text-[11px] text-[var(--color-wardian-warning)] shadow-sm" role="status">
          <span>Some workflows are omitted because the catalog is limited to the first 500; pages are capped at 500.</span>{' '}
          {workflowsNextOffset !== null && (
            <button type="button" className="font-semibold underline" onClick={() => void loadMoreWorkflows()}>
              Load next page
            </button>
          )}
        </div>
      )}
      {rendererActive ? <GardenCanvas
        agentUnits={agentUnits}
        workflowUnits={workflowUnits}
        selectedKey={activeSelectionKey}
        highlightedAgentIds={highlightedAgentIds}
        terrainCells={terrain.cells}
        terrainDistricts={layout.districts}
        terrainPaint={changes.paint}
        highlightedPaths={highlightedPaths}
        onSelectPath={handleSelectPath}
        onOpenPath={openPath}
        onViewportChange={setViewport}
        onSelect={(ref) => {
          const key = unitKey(ref);
          setSelectedKey(key);
          // Skills and ground are not placed, so there is no position for the
          // scene to remember and nothing for `visit` to keep from drifting.
          if (ref.kind !== "skill" && ref.kind !== "path") visitUnit(key);
          if (ref.kind === "agent") {
            onSelectionChange(new Set([ref.id]));
          }
        }}
        onOpenAgent={(agentId) => (onOpenAgent ?? onOpenAgentInGrid)?.(agentId)}
        onOpenSkill={(glyph) => openLibraryAt("skills", glyph.entryRef)}
        onMoveUnit={(key, x, y) => {
          // A drag is a pin, stored relative to the unit's district so the
          // placement survives the district being relocated on the grid.
          const where = placement.get(key);
          if (!where) return;
          pinUnit(key, where.districtId, clampToDistrict({ x, y }, where), where.districtOrigin);
        }}
        onResetLayout={() => {
          resetLayout();
          setSelectedKey(null);
        }}
      /> : (
        // A visible surface reaches this branch too: the renderer is mounted
        // from an effect so the surface frame paints first, and the Konva chunk
        // is fetched after that. Saying "paused while hidden" there would be
        // wrong, so the two states read differently.
        <div className="flex flex-1 items-center justify-center text-sm text-muted">
          {visibility === "hidden"
            ? "Garden renderer paused while hidden"
            : "Preparing the garden…"}
        </div>
      )}
    </div>
  );
};

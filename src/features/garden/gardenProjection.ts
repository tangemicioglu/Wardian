import type { AgentGraphProjection, CommEdgeState } from "../graph/graphProjection";
import type { AgentTeam } from "../../layout/watchlist/types";
import type {
  GardenAgentUnit,
  GardenLibraryUnit,
  GardenPosition,
  GardenWorkflowRunStatus,
  GardenWorkflowUnit,
} from "./garden.types";
import { agentRef, entityKey, libraryEntryRef, workflowRef } from "./entityRef";
import {
  emitAgentFacets,
  emitClassFacets,
  emitPromptFacets,
  emitSkillFacets,
  emitWorkflowFacets,
} from "./facets";
import {
  COMMONS_DISTRICT_ID,
  districtId,
  resolveAgentDistrict,
  resolveEntityDistrict,
} from "./districts";
import type { GardenLibraryInput } from "./useGardenLibrary";
import { interactionWeight, personalizedPageRank } from "./metric";
import { layoutGarden, type LayoutEntity } from "./gardenLayout";
import type { GardenScene } from "./gardenScene";

export interface GardenWorkflowInput {
  id: string;
  label: string;
  runStatus: GardenWorkflowRunStatus;
  nodeCount: number;
}

/**
 * Footprints used for overlap removal. Approximate on purpose: the constraint
 * solver only needs a bound, and measuring rendered text every frame would make
 * geometry depend on font loading.
 */
const AGENT_UNIT_SIZE = { width: 96, height: 42 };
const WORKFLOW_UNIT_SIZE = { width: 120, height: 52 };
const LIBRARY_UNIT_SIZE = { width: 104, height: 34 };

export interface GardenProjectionInput {
  /** Display data — labels, status, colour — plus communication edges. */
  projection: AgentGraphProjection;
  teams: readonly AgentTeam[];
  workflows: readonly GardenWorkflowInput[];
  library?: readonly GardenLibraryInput[];
  scene: GardenScene;
  now?: number;
}

export interface UnitPlacement {
  districtId: string;
  /** District origin, the frame a pin for this unit is stored against. */
  districtOrigin: GardenPosition;
}

export interface GardenProjectionResult {
  /** entityKey -> world position. Display fields are attached separately. */
  positions: Map<string, GardenPosition>;
  /** Scene with refreshed district cells and warm-start positions. */
  scene: GardenScene;
  /** Pins stranded by a district change; the view offers a re-place. */
  stalePinKeys: string[];
  /** entityKey -> where it sits, so a drag can become a district-relative pin. */
  placement: Map<string, UnitPlacement>;
}

/**
 * Project Wardian entities into placed Garden units.
 *
 * Replaces the previous phyllotaxis seeding. That spiral was a stopgap: the
 * graph view's spring-electric positions piled agents on top of each other, so
 * the Garden discarded them for an even spread that carried no meaning at all.
 * Positions now come from the metric pipeline in `gardenLayout.ts`, so distance
 * on screen reflects distance in facet space.
 *
 * Status and colour still come from the graph projection, but they are display
 * channels only — they never reach `layoutGarden`, which is precisely why a
 * status change or a message tick cannot move anything.
 */
export function computeGardenLayout(input: GardenProjectionInput): GardenProjectionResult {
  const teamsByAgent = new Map<string, string[]>();
  for (const team of input.teams) {
    for (const agentId of team.agentIds) {
      const existing = teamsByAgent.get(agentId);
      if (existing) existing.push(team.id);
      else teamsByAgent.set(agentId, [team.id]);
    }
  }

  const entities: LayoutEntity[] = [];
  const districtByAgentId = new Map<string, string>();

  for (const node of input.projection.nodes) {
    const ref = agentRef(node.id);
    const teamIds = teamsByAgent.get(node.id);
    const district = districtId(resolveAgentDistrict(node.agent, { teamIds }));
    districtByAgentId.set(node.id, district);
    entities.push({
      ref,
      facets: emitAgentFacets(node.agent, ref, { teamIds }),
      districtId: district,
      ...AGENT_UNIT_SIZE,
    });
  }

  // Library assets are placed by where they are *deployed*, which is a canonical
  // record rather than an inference. An asset with no deployment has no
  // defensible home and lands in the commons instead of being guessed into
  // someone's district.
  for (const entry of input.library ?? []) {
    const ref = libraryEntryRef(entry.entryRef);
    if (!ref) continue;
    const facets =
      entry.kind === "skill"
        ? emitSkillFacets(ref, { tags: entry.tags, deployments: entry.deployments })
        : entry.kind === "prompt"
          ? emitPromptFacets(ref, entry.tags)
          : emitClassFacets(ref, {
              memberAgentIds: entry.deployments
                .filter((deployment) => deployment.targetType === "agent")
                .map((deployment) => deployment.targetId),
            });
    entities.push({
      ref,
      facets,
      districtId: resolveEntityDistrict(facets.tokens, districtByAgentId),
      ...LIBRARY_UNIT_SIZE,
    });
  }

  for (const workflow of input.workflows) {
    const ref = workflowRef(workflow.id);
    entities.push({
      ref,
      facets: emitWorkflowFacets(ref),
      // A blueprint carries no durable binding to an agent or team until a run
      // assigns roles, so it lives in the commons rather than being guessed
      // into someone's district.
      districtId: COMMONS_DISTRICT_ID,
      ...WORKFLOW_UNIT_SIZE,
    });
  }

  // Interaction affinity stays confined to the agent subgraph: it is dense
  // O(n^2) per source, and blueprints relate through facets instead.
  const ppr = personalizedPageRank(
    input.projection.nodes.map((node) => entityKey(agentRef(node.id))),
    input.projection.commEdges.map((edge) => ({
      source: entityKey(agentRef(edge.source)),
      target: entityKey(agentRef(edge.target)),
      weight: interactionWeight({
        manual: edge.origin === "manual",
        // Quantized deliberately. `CommunicationEdge.recency` is continuous and
        // recomputed against the wall clock, so feeding it in directly would
        // make every distance drift as conversations age and the map would
        // breathe with no user-visible cause. The three activity states are
        // discrete and change rarely, which is the granularity geometry should
        // react to.
        recency: ACTIVITY_WEIGHT[edge.state],
      }),
    })),
  );

  const result = layoutGarden({ entities, scene: input.scene, ppr, now: input.now });

  return {
    positions: new Map(result.units.map((unit) => [unit.key, unit.position])),
    scene: result.scene,
    stalePinKeys: result.stalePinKeys,
    placement: new Map<string, UnitPlacement>(
      result.units.map((unit) => [
        unit.key,
        {
          districtId: unit.districtId,
          districtOrigin: result.districtOrigins.get(unit.districtId) ?? { x: 0, y: 0 },
        },
      ]),
    ),
  };
}

const ACTIVITY_WEIGHT: Record<CommEdgeState, number> = {
  ongoing: 1,
  recent: 0.6,
  dormant: 0.2,
};

/**
 * Stable digest of everything that legitimately affects geometry.
 *
 * The Garden re-renders whenever telemetry ticks, because status and colour are
 * live. Keying the layout on the projection's *identity* would therefore rerun
 * the whole pipeline on every tick — and since stress majorization stops at a
 * tolerance rather than an exact optimum, each rerun advances convergence and
 * nudges every unit a few pixels. The map would drift continuously for reasons
 * the user cannot see.
 *
 * Keying on this digest instead means a status change is a repaint and nothing
 * more. Note what is absent: status, colour, telemetry, selection, and the
 * continuous edge recency.
 */
export function gardenLayoutSignature(
  projection: AgentGraphProjection,
  teams: readonly AgentTeam[],
  workflows: readonly GardenWorkflowInput[],
  library: readonly GardenLibraryInput[] = [],
): string {
  const agents = [...projection.nodes]
    .sort((left, right) => left.id.localeCompare(right.id))
    .map((node) =>
      [
        node.id,
        node.agent.folder ?? "",
        node.agent.git_worktree_source ?? "",
        node.agent.git_worktree_folder ?? "",
        node.agent.agent_class ?? "",
        node.agent.provider ?? "",
        node.agent.model ?? "",
        (node.agent.include_directories ?? []).join(","),
        (node.agent.system_include_directories ?? []).join(","),
        (node.agent.allowed_mcp_server_names ?? []).join(","),
        (node.agent.extensions ?? []).join(","),
      ].join("|"),
    )
    .join(";");

  const teamKey = [...teams]
    .sort((left, right) => left.id.localeCompare(right.id))
    .map((team) => `${team.id}:${[...team.agentIds].sort().join(",")}`)
    .join(";");

  const workflowKey = [...workflows]
    .map((workflow) => workflow.id)
    .sort()
    .join(",");

  const edgeKey = [...projection.commEdges]
    .map((edge) => `${edge.id}:${edge.origin}:${edge.state}`)
    .sort()
    .join(";");

  // Deployment targets move a library asset between districts, so they belong in
  // the signature. Labels and tags do not — renaming a skill repaints its unit
  // but must not relayout the map.
  const libraryKey = [...library]
    .map(
      (entry) =>
        `${entry.entryRef}:${entry.deployments
          .map((deployment) => `${deployment.targetType}/${deployment.targetId}/${deployment.linked}`)
          .sort()
          .join(",")}`,
    )
    .sort()
    .join(";");

  return [agents, teamKey, workflowKey, edgeKey, libraryKey].join("#");
}

/** Attach live display fields to computed positions. */
export function buildAgentUnits(
  projection: AgentGraphProjection,
  positions: ReadonlyMap<string, GardenPosition>,
): GardenAgentUnit[] {
  return projection.nodes.map((node) => ({
    ref: { kind: "agent", id: node.id },
    label: node.label,
    status: node.status,
    color: node.color,
    position: positions.get(entityKey(agentRef(node.id))) ?? { x: 0, y: 0 },
  }));
}

export function buildLibraryUnits(
  library: readonly GardenLibraryInput[],
  positions: ReadonlyMap<string, GardenPosition>,
): GardenLibraryUnit[] {
  const units: GardenLibraryUnit[] = [];
  for (const entry of library) {
    const ref = libraryEntryRef(entry.entryRef);
    if (!ref) continue;
    const position = positions.get(entityKey(ref));
    if (!position) continue;
    units.push({
      ref: { kind: entry.kind, id: ref.id },
      label: entry.label,
      deploymentCount: entry.deployments.length,
      hasCopiedDeployment: entry.deployments.some((deployment) => !deployment.linked),
      position,
    });
  }
  return units;
}

export function buildWorkflowUnits(
  workflows: readonly GardenWorkflowInput[],
  positions: ReadonlyMap<string, GardenPosition>,
): GardenWorkflowUnit[] {
  return workflows.map((workflow) => ({
    ref: { kind: "workflow", id: workflow.id },
    label: workflow.label,
    runStatus: workflow.runStatus,
    nodeCount: workflow.nodeCount,
    position: positions.get(entityKey(workflowRef(workflow.id))) ?? { x: 0, y: 0 },
  }));
}

import type { AgentGraphProjection } from "../graph/graphProjection";
import type { AgentTeam } from "../../layout/watchlist/types";
import type {
  GardenAgentUnit,
  GardenPosition,
  GardenWorkflowRunStatus,
  GardenWorkflowUnit,
} from "./garden.types";
import { agentRef, entityKey, workflowRef } from "./entityRef";
import { emitAgentFacets, emitWorkflowFacets } from "./facets";
import { COMMONS_DISTRICT_ID, districtId, resolveAgentDistrict } from "./districts";
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

export interface GardenProjectionInput {
  /** Display data — labels, status, colour — plus communication edges. */
  projection: AgentGraphProjection;
  teams: readonly AgentTeam[];
  workflows: readonly GardenWorkflowInput[];
  scene: GardenScene;
  now?: number;
}

export interface UnitPlacement {
  districtId: string;
  /** District origin, the frame a pin for this unit is stored against. */
  districtOrigin: GardenPosition;
}

export interface GardenProjectionResult {
  agentUnits: GardenAgentUnit[];
  workflowUnits: GardenWorkflowUnit[];
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
export function buildGardenUnits(input: GardenProjectionInput): GardenProjectionResult {
  const teamsByAgent = new Map<string, string[]>();
  for (const team of input.teams) {
    for (const agentId of team.agentIds) {
      const existing = teamsByAgent.get(agentId);
      if (existing) existing.push(team.id);
      else teamsByAgent.set(agentId, [team.id]);
    }
  }

  const entities: LayoutEntity[] = [];

  for (const node of input.projection.nodes) {
    const ref = agentRef(node.id);
    const teamIds = teamsByAgent.get(node.id);
    entities.push({
      ref,
      facets: emitAgentFacets(node.agent, ref, { teamIds }),
      districtId: districtId(resolveAgentDistrict(node.agent, { teamIds })),
      ...AGENT_UNIT_SIZE,
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
      weight: interactionWeight({ manual: edge.origin === "manual", recency: edge.recency }),
    })),
  );

  const result = layoutGarden({ entities, scene: input.scene, ppr, now: input.now });
  const positionOf = new Map(result.units.map((unit) => [unit.key, unit.position]));

  const agentUnits: GardenAgentUnit[] = input.projection.nodes.map((node) => ({
    ref: { kind: "agent", id: node.id },
    label: node.label,
    status: node.status,
    color: node.color,
    position: positionOf.get(entityKey(agentRef(node.id))) ?? { x: 0, y: 0 },
  }));

  const workflowUnits: GardenWorkflowUnit[] = input.workflows.map((workflow) => ({
    ref: { kind: "workflow", id: workflow.id },
    label: workflow.label,
    runStatus: workflow.runStatus,
    nodeCount: workflow.nodeCount,
    position: positionOf.get(entityKey(workflowRef(workflow.id))) ?? { x: 0, y: 0 },
  }));

  const placement = new Map<string, UnitPlacement>(
    result.units.map((unit) => [
      unit.key,
      {
        districtId: unit.districtId,
        districtOrigin: result.districtOrigins.get(unit.districtId) ?? { x: 0, y: 0 },
      },
    ]),
  );

  return {
    agentUnits,
    workflowUnits,
    scene: result.scene,
    stalePinKeys: result.stalePinKeys,
    placement,
  };
}

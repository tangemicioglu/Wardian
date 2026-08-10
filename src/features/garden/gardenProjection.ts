import type { AgentGraphProjection, CommEdgeState } from "../graph/graphProjection";
import type { AgentTeam } from "../../layout/watchlist/types";
import type {
  GardenAgentUnit,
  GardenPosition,
  GardenWorkflowRunStatus,
  GardenWorkflowUnit,
} from "./garden.types";
import { agentRef, entityKey, normalizeEntityPath, workflowRef } from "./entityRef";
import { emitAgentFacets, emitWorkflowFacets } from "./facets";
import {
  buildDistrictAffinity,
  districtId,
  resolveAgentDistrict,
  resolveEntityDistrict,
} from "./districts";
import type { GardenSkillInput } from "./useGardenSkills";
import { buildSkillCrowns, crownExtent, type GardenSkillGlyph } from "./skillGlyphs";
import { interactionWeight, personalizedPageRank } from "./metric";
import { layoutGarden, type LayoutEntity } from "./gardenLayout";
import type { GardenScene } from "./gardenScene";
import type { TerrainDistrict } from "./terrain";

export interface GardenWorkflowInput {
  id: string;
  label: string;
  runStatus: GardenWorkflowRunStatus;
  nodeCount: number;
  /**
   * Concrete agent ids the workflow is bound to: `agent_ref` node fields that
   * name an agent, pooled with the agents its schedules deploy it onto.
   */
  agentIds?: readonly string[];
  /** Roles the blueprint leaves open, e.g. `evolver` from `role:evolver`. */
  roleNames?: readonly string[];
  /** Agent classes the blueprint requires, e.g. `Coder` from `class:Coder`. */
  classNames?: readonly string[];
  /** Directories named by `path` node fields, e.g. a shell node's `cwd`. */
  workspacePaths?: readonly string[];
  /** Section-relative library folder, e.g. `trident`. */
  libraryFolder?: string | null;
}

/**
 * Footprints used for overlap removal. Approximate on purpose: the constraint
 * solver only needs a bound, and measuring rendered text every frame would make
 * geometry depend on font loading.
 */
const AGENT_UNIT_SIZE = { width: 96, height: 42 };
const WORKFLOW_UNIT_SIZE = { width: 120, height: 52 };

/**
 * Footprint of an agent carrying `crownLength` skills.
 *
 * The crown is drawn above the dot, so it extends the box upward and, once it
 * is wide enough, sideways. It is measured at full detail even though the crown
 * is truncated when zoomed out — a footprint that shrank on zoom would let the
 * map rearrange itself in response to the viewport, which is exactly what the
 * stability contract forbids.
 */
function agentFootprint(crownLength: number): { width: number; height: number } {
  const extent = crownExtent(crownLength);
  return {
    width: Math.max(AGENT_UNIT_SIZE.width, 2 * extent),
    height: AGENT_UNIT_SIZE.height + extent,
  };
}

export interface GardenProjectionInput {
  /** Display data — labels, status, colour — plus communication edges. */
  projection: AgentGraphProjection;
  teams: readonly AgentTeam[];
  workflows: readonly GardenWorkflowInput[];
  skills?: readonly GardenSkillInput[];
  scene: GardenScene;
  now?: number;
}

export interface UnitPlacement {
  districtId: string;
  /** District origin, the frame a pin for this unit is stored against. */
  districtOrigin: GardenPosition;
  /**
   * How far the district reaches from that origin. A drag is held inside it, so
   * a unit cannot be dropped into a neighbour's territory and read as belonging
   * there.
   */
  districtRadius: number;
}

export interface GardenProjectionResult {
  /** entityKey -> world position. Display fields are attached separately. */
  positions: Map<string, GardenPosition>;
  /**
   * agentId -> its skill glyphs, most distinctive first.
   *
   * Produced here rather than in the view because the same resolution feeds the
   * agent's `skill:` facets, and the two must not be able to disagree about
   * which skills an agent has.
   */
  crowns: Map<string, GardenSkillGlyph[]>;
  /** Scene with refreshed district cells and warm-start positions. */
  scene: GardenScene;
  /** Pins stranded by a district change; the view offers a re-place. */
  stalePinKeys: string[];
  /** entityKey -> where it sits, so a drag can become a district-relative pin. */
  placement: Map<string, UnitPlacement>;
  /**
   * districtId -> the territory it occupies, plus the workspace roots its
   * agents work in.
   *
   * Published so terrain can be drawn beneath the units without re-deriving
   * district membership. Roots come from the same `git_worktree_folder ??
   * folder` value `resolveAgentDistrict` partitions on, so ground membership
   * and district membership cannot disagree.
   */
  districts: Map<string, TerrainDistrict>;
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

  // Skills are attributes of agents rather than units of their own, so they
  // enter the pipeline only as agent facets. That is a strengthening, not a
  // concession: `skill:<entry_ref>` is rare and therefore high-IDF, so two
  // agents carrying the same unusual skill are genuinely pulled together —
  // whereas a skill *unit* had to sit at the centroid of its carriers, where it
  // was near none of them. See `skillGlyphs.ts`.
  //
  // Crowns supply the facets because they already resolve class-inherited and
  // global deployments down to the agents they actually reach, which a raw scan
  // of `deployments` does not. A global skill lands on every agent, so its IDF
  // is exactly 0 and it costs nothing — the corpus regulates it without a rule.
  const crowns = buildSkillCrowns(
    input.skills ?? [],
    input.projection.nodes.map((node) => ({
      id: node.id,
      agentClass: node.agent.agent_class,
    })),
  );

  const districtByAgentId = new Map<string, string>();
  // districtId -> the workspace roots its agents actually operate in. A team
  // district spanning several repositories keeps all of them: that overlap is
  // the thing the malleable-garden spec asked Garden to make visible rather
  // than nest away.
  const rootsByDistrict = new Map<string, Set<string>>();
  for (const node of input.projection.nodes) {
    const ref = agentRef(node.id);
    const teamIds = teamsByAgent.get(node.id);
    const district = districtId(resolveAgentDistrict(node.agent, { teamIds }));
    districtByAgentId.set(node.id, district);
    const root =
      normalizeEntityPath(node.agent.git_worktree_folder) ?? normalizeEntityPath(node.agent.folder);
    if (root) {
      const existing = rootsByDistrict.get(district);
      if (existing) existing.add(root);
      else rootsByDistrict.set(district, new Set([root]));
    }
    const crown = crowns.get(node.id) ?? [];
    entities.push({
      ref,
      facets: emitAgentFacets(node.agent, ref, {
        teamIds,
        deployedSkillRefs: crown.map((glyph) => glyph.entryRef),
      }),
      districtId: district,
      ...agentFootprint(crown.length),
    });
  }

  // Evidence for placing everything that is not an agent. Agents are already
  // placed by a canonical rule, so they are the fixed points the rest is
  // measured against.
  const affinity = buildDistrictAffinity(
    entities.map((entity) => ({ tokens: entity.facets.tokens, districtId: entity.districtId })),
  );

  for (const workflow of input.workflows) {
    const ref = workflowRef(workflow.id);
    // A blueprint binds no agent until a run assigns roles, which is why these
    // all used to sit in the commons. But a blueprint is not short of evidence:
    // an `agent_ref` field is an outright binding, and a `path` field — a shell
    // node's `cwd`, say — names the directory the workflow operates on, which
    // the agents living there also carry. See `workflowContext.ts`.
    const facets = emitWorkflowFacets(ref, {
      assignedAgentIds: workflow.agentIds,
      // Roles and classes cannot place a workflow — they name a kind of agent,
      // not one — but they are strong evidence of kinship between workflows, and
      // that is what the affinity fallback reads when no binding exists.
      roleNames: workflow.roleNames,
      classNames: workflow.classNames,
      workspacePaths: workflow.workspacePaths,
      libraryFolder: workflow.libraryFolder,
    });
    entities.push({
      ref,
      facets,
      // Falls back to the commons when the evidence is too thin to act on,
      // rather than guessing the workflow into someone's district.
      districtId: resolveEntityDistrict(facets.tokens, districtByAgentId, affinity),
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

  const territory = new Map<string, TerrainDistrict>();
  for (const [district, roots] of rootsByDistrict) {
    territory.set(district, {
      roots: [...roots].sort(),
      origin: result.districtOrigins.get(district) ?? { x: 0, y: 0 },
      radius: result.districtExtents.get(district) ?? 0,
    });
  }

  return {
    positions: new Map(result.units.map((unit) => [unit.key, unit.position])),
    districts: territory,
    crowns,
    scene: result.scene,
    stalePinKeys: result.stalePinKeys,
    placement: new Map<string, UnitPlacement>(
      result.units.map((unit) => [
        unit.key,
        {
          districtId: unit.districtId,
          districtOrigin: result.districtOrigins.get(unit.districtId) ?? { x: 0, y: 0 },
          districtRadius: result.districtExtents.get(unit.districtId) ?? 0,
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
  skills: readonly GardenSkillInput[] = [],
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

  // Everything that can move a workflow: its identity plus the evidence that
  // places it. Run status and node count are display, and stay out.
  const workflowKey = [...workflows]
    .map((workflow) =>
      [
        workflow.id,
        (workflow.agentIds ?? []).join(","),
        (workflow.roleNames ?? []).join(","),
        (workflow.classNames ?? []).join(","),
        (workflow.workspacePaths ?? []).join(","),
        workflow.libraryFolder ?? "",
      ].join("|"),
    )
    .sort()
    .join(";");

  const edgeKey = [...projection.commEdges]
    .map((edge) => `${edge.id}:${edge.origin}:${edge.state}`)
    .sort()
    .join(";");

  // Deployments are agent facets now, so they still move things and still
  // belong here. `linked` does not: a copy and a junction are the same tie for
  // distance purposes and differ only in how the glyph is stroked, so including
  // it would relayout the map when a deployment merely fell back to a copy.
  // Labels and tags are likewise absent — renaming a skill repaints a glyph.
  const skillKey = [...skills]
    .map(
      (skill) =>
        `${skill.entryRef}:${skill.deployments
          .map((deployment) => `${deployment.targetType}/${deployment.targetId}`)
          .sort()
          .join(",")}`,
    )
    .sort()
    .join(";");

  return [agents, teamKey, workflowKey, edgeKey, skillKey].join("#");
}

/** Attach live display fields to computed positions. */
export function buildAgentUnits(
  projection: AgentGraphProjection,
  positions: ReadonlyMap<string, GardenPosition>,
  crowns: ReadonlyMap<string, GardenSkillGlyph[]> = new Map(),
): GardenAgentUnit[] {
  return projection.nodes.map((node) => ({
    ref: { kind: "agent", id: node.id },
    label: node.label,
    status: node.status,
    color: node.color,
    position: positions.get(entityKey(agentRef(node.id))) ?? { x: 0, y: 0 },
    crown: crowns.get(node.id) ?? [],
  }));
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

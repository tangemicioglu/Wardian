import type { AgentGraphProjection, CommEdgeState } from "../graph/graphProjection";
import type { AgentConfig, AgentReachEntry } from "../../types";
import type { AgentTeam } from "../../layout/watchlist/types";
import type {
  GardenAgentUnit,
  GardenPosition,
  GardenAutomationRunStatus,
  GardenAutomationUnit,
} from "./garden.types";
import { agentRef, entityKey, normalizeEntityPath, automationRef } from "./entityRef";
import { emitAgentFacets, emitAutomationFacets } from "./facets";
import {
  buildDistrictAffinity,
  districtId,
  reachTier,
  resolveAgentDistrict,
  resolveEntityDistrict,
} from "./districts";
import type { GardenSkillInput } from "./useGardenSkills";
import { buildSkillCrowns, crownExtent, type GardenSkillGlyph } from "./skillGlyphs";
import { interactionWeight, personalizedPageRank } from "./metric";
import { layoutGarden, type LayoutEntity } from "./gardenLayout";
import type { GardenScene } from "./gardenScene";
import { groundRadiusFor, quantizeAnchor, type TerrainDistrict } from "./terrain";

export interface GardenAutomationInput {
  id: string;
  label: string;
  runStatus: GardenAutomationRunStatus;
  nodeCount: number;
  /**
   * Concrete agent ids the automation is bound to: `agent_ref` node fields that
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
const AUTOMATION_UNIT_SIZE = { width: 120, height: 52 };

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
  automations: readonly GardenAutomationInput[];
  skills?: readonly GardenSkillInput[];
  scene: GardenScene;
  /** Workspace roots each agent has written under; see `useGardenReach`. */
  reach?: readonly AgentReachEntry[];
  now?: number;
}

/**
 * Workspace roots the roster works in, sorted and de-duplicated.
 *
 * Derived from the agents rather than from a computed layout, and the direction
 * is the point: reach is a layout *input*, so reading roots off the layout's
 * districts would close a loop — roots would depend on a layout that depended on
 * the reach those roots produced. The same `git_worktree_folder ?? folder` value
 * `computeGardenLayout` districts on, so the two cannot disagree about what a
 * root is.
 */
export function agentWorkspaceRoots(agents: readonly AgentConfig[]): string[] {
  const roots = new Set<string>();
  for (const agent of agents) {
    const root =
      normalizeEntityPath(agent.git_worktree_folder) ?? normalizeEntityPath(agent.folder);
    if (root) roots.add(root);
  }
  return [...roots].sort();
}

/**
 * Reach tier per district: how many *other* districts its agents write into.
 *
 * A root can belong to more than one district — a team district and the
 * workspace district of an untamed agent in the same repository both claim it —
 * so a write counts toward every district that holds the root, minus the writer's
 * own. That is the honest reading: the write really did land in territory those
 * districts occupy.
 */
export function districtReachTiers(
  reach: readonly AgentReachEntry[],
  districtByAgentId: ReadonlyMap<string, string>,
  rootsByDistrict: ReadonlyMap<string, ReadonlySet<string>>,
): Map<string, number> {
  const districtsByRoot = new Map<string, Set<string>>();
  for (const [district, roots] of rootsByDistrict) {
    for (const root of roots) {
      const existing = districtsByRoot.get(root);
      if (existing) existing.add(district);
      else districtsByRoot.set(root, new Set([district]));
    }
  }

  const reachedByDistrict = new Map<string, Set<string>>();
  for (const entry of reach) {
    const home = districtByAgentId.get(entry.agent_id);
    if (!home) continue;
    for (const rawRoot of entry.roots) {
      const root = normalizeEntityPath(rawRoot);
      if (!root) continue;
      for (const district of districtsByRoot.get(root) ?? []) {
        if (district === home) continue;
        const existing = reachedByDistrict.get(home);
        if (existing) existing.add(district);
        else reachedByDistrict.set(home, new Set([district]));
      }
    }
  }

  return new Map([...reachedByDistrict].map(([district, reached]) => [
    district,
    reachTier(reached.size),
  ]));
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
  /**
   * districtId -> reach tier, for districts that reach past their own territory.
   *
   * Published so the view can say that centrality means something on this map.
   * An arrangement that encodes a claim nobody can read is not an improvement
   * over one that encodes nothing.
   */
  reachTiers: Map<string, number>;
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
  // Which root each agent works in, so the ground can be laid out under the
  // agents rather than beside them. Agents only: an automation has no workspace.
  const rootByUnitKey = new Map<string, string>();
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
      rootByUnitKey.set(entityKey(ref), root);
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

  for (const automation of input.automations) {
    const ref = automationRef(automation.id);
    // A blueprint binds no agent until a run assigns roles, which is why these
    // all used to sit in the commons. But a blueprint is not short of evidence:
    // an `agent_ref` field is an outright binding, and a `path` field — a shell
    // node's `cwd`, say — names the directory the automation operates on, which
    // the agents living there also carry. See `automationContext.ts`.
    const facets = emitAutomationFacets(ref, {
      assignedAgentIds: automation.agentIds,
      // Roles and classes cannot place an automation — they name a kind of agent,
      // not one — but they are strong evidence of kinship between automations, and
      // that is what the affinity fallback reads when no binding exists.
      roleNames: automation.roleNames,
      classNames: automation.classNames,
      workspacePaths: automation.workspacePaths,
      libraryFolder: automation.libraryFolder,
    });
    entities.push({
      ref,
      facets,
      // Falls back to the commons when the evidence is too thin to act on,
      // rather than guessing the automation into someone's district.
      districtId: resolveEntityDistrict(facets.tokens, districtByAgentId, affinity),
      ...AUTOMATION_UNIT_SIZE,
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

  const reachTiers = districtReachTiers(input.reach ?? [], districtByAgentId, rootsByDistrict);
  const result = layoutGarden({
    entities,
    scene: input.scene,
    ppr,
    reachTiers,
    now: input.now,
  });

  // Ground is sized against the free space around each district, measured over
  // *every* district and not only the ones carrying terrain: a neighbour with
  // no workspace still holds units that the ground must not be drawn over.
  const origins = [...result.districtOrigins.entries()];

  // Where each root's agents settled, district-relative, so a district spanning
  // several repositories can put each one's ground under the agents that work
  // in it. Positions only enter as an *ordering* of cells that already exist —
  // they can no more change a rect than the change set can.
  const anchorTotals = new Map<string, Map<string, { x: number; y: number; count: number }>>();
  for (const unit of result.units) {
    const root = rootByUnitKey.get(unit.key);
    if (!root) continue;
    const origin = result.districtOrigins.get(unit.districtId) ?? { x: 0, y: 0 };
    let byRoot = anchorTotals.get(unit.districtId);
    if (!byRoot) {
      byRoot = new Map();
      anchorTotals.set(unit.districtId, byRoot);
    }
    const total = byRoot.get(root) ?? { x: 0, y: 0, count: 0 };
    total.x += unit.position.x - origin.x;
    total.y += unit.position.y - origin.y;
    total.count += 1;
    byRoot.set(root, total);
  }

  const territory = new Map<string, TerrainDistrict>();
  for (const [district, roots] of rootsByDistrict) {
    const origin = result.districtOrigins.get(district) ?? { x: 0, y: 0 };
    let nearest = Number.POSITIVE_INFINITY;
    for (const [other, point] of origins) {
      if (other === district) continue;
      nearest = Math.min(nearest, Math.hypot(point.x - origin.x, point.y - origin.y));
    }
    const totals = anchorTotals.get(district);
    const anchors = totals
      ? new Map(
          [...totals].map(([root, total]) => [
            root,
            quantizeAnchor({ x: total.x / total.count, y: total.y / total.count }),
          ]),
        )
      : undefined;
    territory.set(district, {
      roots: [...roots].sort(),
      anchors,
      origin,
      radius: groundRadiusFor(result.districtExtents.get(district) ?? 0, nearest),
    });
  }

  return {
    positions: new Map(result.units.map((unit) => [unit.key, unit.position])),
    districts: territory,
    reachTiers,
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
  automations: readonly GardenAutomationInput[],
  skills: readonly GardenSkillInput[] = [],
  reach: readonly AgentReachEntry[] = [],
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

  // Everything that can move an automation: its identity plus the evidence that
  // places it. Run status and node count are display, and stay out.
  const automationKey = [...automations]
    .map((automation) =>
      [
        automation.id,
        (automation.agentIds ?? []).join(","),
        (automation.roleNames ?? []).join(","),
        (automation.classNames ?? []).join(","),
        (automation.workspacePaths ?? []).join(","),
        automation.libraryFolder ?? "",
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

  // Reach seats districts, so it belongs here — and it is safe to include for
  // the same reason it is safe to lay out on: it is history, fetched once, and
  // it changes only when an agent writes somewhere it never had. Digested at
  // full resolution rather than as tiers because the tier is derived from
  // district membership, which this digest cannot see.
  const reachKey = [...reach]
    .map((entry) => `${entry.agent_id}:${[...entry.roots].sort().join(",")}`)
    .sort()
    .join(";");

  return [agents, teamKey, automationKey, edgeKey, skillKey, reachKey].join("#");
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

export function buildAutomationUnits(
  automations: readonly GardenAutomationInput[],
  positions: ReadonlyMap<string, GardenPosition>,
): GardenAutomationUnit[] {
  return automations.map((automation) => ({
    ref: { kind: "automation", id: automation.id },
    label: automation.label,
    runStatus: automation.runStatus,
    nodeCount: automation.nodeCount,
    position: positions.get(entityKey(automationRef(automation.id))) ?? { x: 0, y: 0 },
  }));
}

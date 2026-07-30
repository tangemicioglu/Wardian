/**
 * Garden layout pipeline: entities in, placed units out.
 *
 *   facets -> IDF corpus -> districts -> per-district k-NN -> SMACOF -> VPSC
 *
 * ## The district is the unit of recomputation
 *
 * Every superlinear stage runs *inside* a district: the k-NN build is all-pairs
 * and stress majorization is iterative, so both depend on district size, which
 * `MAX_DISTRICT_MEMBERS` caps. Insertion therefore costs one district, not the
 * map. Nothing here is global except facet extraction and district placement,
 * which are linear and near-constant respectively.
 *
 * ## Telemetry is not an input
 *
 * `LayoutInput` accepts no status, no telemetry, no lens state. That is enforced
 * by the type rather than by discipline, because the invariant it protects is
 * the whole design: status changes, message ticks, lens toggles, and window
 * resizes must change colour, tint, and emphasis — never geometry. A map whose
 * shape shifts when a filter toggles is asserting something it cannot back, and
 * `docs/specs/2026-07-14-entity-oriented-agent-semantics.md` is explicit that
 * Garden spatial proximity is interpretive and never a claim about system state.
 */

import type { GardenPosition } from "./garden.types";
import { entityKey, type EntityRef } from "./entityRef";
import {
  buildCorpus,
  withPlacement,
  type FacetCorpus,
  type GardenEntityFacets,
} from "./facets";
import {
  COMMONS_DISTRICT_ID,
  MAX_DISTRICT_MEMBERS,
  districtCenter,
  parcelsFor,
  placeDistricts,
  retireDistricts,
  type DistrictLayout,
} from "./districts";
import {
  DEFAULT_KNN,
  DEFAULT_METRIC_WEIGHTS,
  buildNeighbourGraph,
  symmetrizeGraph,
  type CoUseIndex,
  type MetricContext,
  type MetricWeights,
  type PprMatrix,
} from "./metric";
import {
  DEFAULT_LAYOUT_SCALE,
  DEFAULT_MAX_ITERATIONS,
  runSmacof,
  type SmacofNode,
} from "./smacof";
import { removeOverlaps, type UnitBox } from "./vpsc";
import {
  anchoredDistrict,
  driftFor,
  recordPositions,
  resolvePin,
  sceneAnchorWeights,
  stalePins,
  type GardenScene,
} from "./gardenScene";

/** World-space gap between district cells. */
export const DISTRICT_SPACING = 720;

export interface LayoutEntity {
  ref: EntityRef;
  facets: GardenEntityFacets;
  /** Canonical district for this entity, resolved by the caller. */
  districtId: string;
  /** Footprint used for overlap removal. */
  width: number;
  height: number;
}

export interface LayoutInput {
  entities: readonly LayoutEntity[];
  scene: GardenScene;
  /** Interaction affinity, agent subgraph only. */
  ppr?: PprMatrix;
  coUse?: CoUseIndex;
  ignoredPairs?: ReadonlySet<string>;
  suppressedSeedPairs?: ReadonlySet<string>;
  weights?: MetricWeights;
  scale?: number;
  knn?: number;
  maxIterations?: number;
  now?: number;
}

export interface PlacedUnit {
  ref: EntityRef;
  key: string;
  districtId: string;
  parcelId: string;
  position: GardenPosition;
  pinned: boolean;
}

export interface LayoutResult {
  units: PlacedUnit[];
  /** Scene with refreshed district cells and warm-start positions. */
  scene: GardenScene;
  /** Entities whose pin no longer matches their district. */
  stalePinKeys: string[];
  /** Pairs still overlapping after constraint solving. Normally empty. */
  residualOverlaps: Array<[string, string]>;
  corpus: FacetCorpus;
  districts: DistrictLayout;
  /**
   * World-space origin per district. Callers need this to convert a drag into a
   * district-relative pin, and it is the same origin pins resolve against.
   */
  districtOrigins: Map<string, GardenPosition>;
}

/**
 * Run the whole pipeline.
 *
 * Deterministic: given the same entities and scene, the output is identical.
 * There is no `Math.random`, no `Date.now` outside the explicit `now`, and every
 * stage sorts its inputs.
 */
export function layoutGarden(input: LayoutInput): LayoutResult {
  const now = input.now ?? Date.now();
  const scale = input.scale ?? DEFAULT_LAYOUT_SCALE;

  // District membership drives placement facets, so resolve it before facets.
  const districtOf = new Map<string, string>(
    input.entities.map((entity) => [entityKey(entity.ref), entity.districtId]),
  );

  // Fold the user's placements into the facet sets. Anchors join the cosine
  // vector; exclusions stay out of it and act as repulsion in the metric.
  const placed = input.entities.map((entity) => {
    const key = entityKey(entity.ref);
    return {
      ...entity,
      facets: withPlacement(entity.facets, {
        anchoredDistrictId: anchoredDistrict(input.scene, key),
        excludedDistrictIds: input.scene.exclusions[key],
      }),
    };
  });

  const corpus = buildCorpus(placed.map((entity) => entity.facets));
  const anchorWeights = sceneAnchorWeights(input.scene, now);

  const context: MetricContext = {
    corpus,
    weights: input.weights ?? DEFAULT_METRIC_WEIGHTS,
    sceneWeights: anchorWeights,
    ppr: input.ppr,
    coUse: input.coUse,
    ignoredPairs: input.ignoredPairs,
    suppressedSeedPairs: input.suppressedSeedPairs,
    districtOf,
  };

  // --- District cells -----------------------------------------------------
  const byDistrict = new Map<string, typeof placed>();
  for (const entity of placed) {
    const members = byDistrict.get(entity.districtId);
    if (members) members.push(entity);
    else byDistrict.set(entity.districtId, [entity]);
  }
  const activeDistricts = [...byDistrict.keys()].sort();

  const placement = placeDistricts(
    input.scene.districts,
    activeDistricts,
    districtSimilarity(byDistrict, context),
  );
  const districts = retireDistricts(placement.layout, activeDistricts, now);

  // --- Per-district layout ------------------------------------------------
  const units: PlacedUnit[] = [];
  const settled = new Map<string, GardenPosition>();
  const residualOverlaps: Array<[string, string]> = [];
  const districtOrigins = new Map<string, GardenPosition>();

  for (const districtId of activeDistricts) {
    const members = byDistrict.get(districtId)!;
    const cell = districts.cells[districtId];
    const origin =
      cell === undefined
        ? // The grid is full. Park the district at the commons rather than
          // dropping its members off the map entirely.
          districtCenter(districts.cells[COMMONS_DISTRICT_ID] ?? 0, {
            order: districts.order,
            spacing: DISTRICT_SPACING,
          })
        : districtCenter(cell, { order: districts.order, spacing: DISTRICT_SPACING });
    districtOrigins.set(districtId, origin);

    // Oversized districts split into parcels so k-NN and SMACOF stay bounded.
    const parcels = parcelsFor(
      districtId,
      members.map((entity) => entityKey(entity.ref)),
      MAX_DISTRICT_MEMBERS,
    );
    const memberByKey = new Map(members.map((entity) => [entityKey(entity.ref), entity]));

    let parcelIndex = 0;
    for (const [parcelId, memberKeys] of parcels) {
      const parcelMembers = memberKeys
        .map((key) => memberByKey.get(key))
        .filter((entity): entity is (typeof placed)[number] => entity !== undefined);
      if (parcelMembers.length === 0) continue;

      // Parcels of one district fan out around its origin so they do not stack.
      const parcelOrigin = offsetForParcel(origin, parcelIndex);
      parcelIndex += 1;

      const graph = symmetrizeGraph(
        buildNeighbourGraph(
          parcelMembers.map((entity) => entity.facets),
          context,
          input.knn ?? DEFAULT_KNN,
        ),
      );

      const nodes: SmacofNode[] = parcelMembers.map((entity) => {
        const key = entityKey(entity.ref);
        // Pins resolve against the *district* origin, never the parcel origin.
        // Parcels are an internal subdivision that exists to bound computation,
        // and their membership shifts as entities come and go — anchoring a
        // user's placement to one would silently relocate it whenever the split
        // changed.
        const pinnedAt = resolvePin(input.scene, key, districtId, origin);
        return {
          key,
          rho: driftFor(input.scene, key, now),
          anchor: pinnedAt ?? input.scene.positions[key],
        };
      });

      const solved = runSmacof(
        { nodes, graph, center: parcelOrigin, scale },
        input.maxIterations ?? DEFAULT_MAX_ITERATIONS,
      );

      const boxes: UnitBox[] = parcelMembers.map((entity) => {
        const key = entityKey(entity.ref);
        return {
          key,
          position: solved.positions.get(key) ?? parcelOrigin,
          width: entity.width,
          height: entity.height,
          // A pinned unit is immovable here too: constraints route around it
          // rather than sliding it off the spot the user chose.
          weight: input.scene.pins[key] ? Infinity : 1,
        };
      });
      const cleared = removeOverlaps(boxes);
      residualOverlaps.push(...cleared.residualOverlaps);

      for (const entity of parcelMembers) {
        const key = entityKey(entity.ref);
        const position = cleared.positions.get(key) ?? parcelOrigin;
        settled.set(key, position);
        units.push({
          ref: entity.ref,
          key,
          districtId,
          parcelId,
          position,
          pinned: Boolean(input.scene.pins[key]),
        });
      }
    }
  }

  units.sort((left, right) => left.key.localeCompare(right.key));

  return {
    units,
    scene: recordPositions({ ...input.scene, districts }, settled),
    stalePinKeys: stalePins(input.scene, districtOf),
    residualOverlaps,
    corpus,
    districts,
    districtOrigins,
  };
}

/**
 * Similarity between two districts, from their members' shared facets.
 *
 * Used only to seat a *new* district beside the ones it most resembles, so a
 * cheap Jaccard over the districts' pooled facet sets is enough; the expensive
 * metric is reserved for placing entities within a district.
 */
function districtSimilarity(
  byDistrict: ReadonlyMap<string, ReadonlyArray<{ facets: GardenEntityFacets }>>,
  context: MetricContext,
): (a: string, b: string) => number {
  const pooled = new Map<string, Set<string>>();
  for (const [districtId, members] of byDistrict) {
    const tokens = new Set<string>();
    for (const member of members) {
      for (const token of member.facets.tokens) {
        // Universal facets carry no information and would make every district
        // look alike.
        if ((context.corpus.df.get(token) ?? 0) >= context.corpus.entityCount) continue;
        tokens.add(token);
      }
    }
    pooled.set(districtId, tokens);
  }

  return (a, b) => {
    const left = pooled.get(a);
    const right = pooled.get(b);
    if (!left || !right || left.size === 0 || right.size === 0) return 0;
    let shared = 0;
    const [shorter, longer] = left.size <= right.size ? [left, right] : [right, left];
    for (const token of shorter) if (longer.has(token)) shared += 1;
    return shared / (left.size + right.size - shared);
  };
}

/**
 * Fan parcels around their district origin on a ring, so a split district reads
 * as one place with several parts rather than as unrelated clusters.
 */
function offsetForParcel(origin: GardenPosition, index: number): GardenPosition {
  if (index === 0) return origin;
  const angle = (Math.PI * 2 * (index - 1)) / 6;
  const radius = DISTRICT_SPACING * 0.32;
  return { x: origin.x + Math.cos(angle) * radius, y: origin.y + Math.sin(angle) * radius };
}

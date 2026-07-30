/**
 * The Garden scene: everything the map owns, and nothing it does not.
 *
 * Per `docs/specs/2026-06-02-malleable-garden.md`, Garden-owned state is
 * deliberately narrow — spatial position, grouping, pins, local annotations —
 * while agent lifecycle, deployments, team membership, and run evidence stay
 * with their canonical owners. This module holds exactly that narrow slice, as
 * a plain serializable object.
 *
 * ## Pins are district-relative
 *
 * A pin stores `(district_id, dx, dy)`, not an absolute point. Absolute pins
 * rot: if a district's cell ever shifts, an absolute pin strands its entity in
 * the wrong neighbourhood and the map starts lying about affiliation. A
 * district-relative offset travels with its district, so the placement stays
 * semantically true.
 *
 * When an entity's *district* changes — an agent moves teams — the pin is
 * explicitly invalidated and surfaced rather than silently honoured (which
 * would misplace it) or silently dropped (which would destroy the user's work).
 *
 * ## Positions are warm-start seeds, not authority
 *
 * `positions` records where each entity settled last time. It exists so the
 * layout's drift penalty has an anchor, which is what makes insertion
 * incremental instead of a full reflow. It is derived state and safe to discard.
 *
 * ## Persistence
 *
 * The malleable-garden spec calls for scenes to be inspectable files under the
 * active Wardian home. This module is deliberately I/O-free and fully
 * serializable so that lands as a storage swap rather than a rewrite; the
 * current caller still persists through the existing browser-side store.
 */

import type { GardenPosition } from "./garden.types";
import { METRIC_VERSION } from "./metric";
import { sceneAnchorToken, sceneFacetWeight, type FacetToken } from "./facets";
import {
  DRIFT_NEW,
  DRIFT_PINNED,
  DRIFT_SETTLED,
  DRIFT_VISITED,
} from "./smacof";
import { createDistrictLayout, type DistrictLayout } from "./districts";

export const GARDEN_SCENE_SCHEMA = 1;

/** How recently an entity must have been visited to resist drift strongly (~1h). */
export const RECENTLY_VISITED_MS = 60 * 60 * 1000;

export interface DistrictRelativePin {
  district_id: string;
  dx: number;
  dy: number;
  placed_at_ms: number;
}

export interface GardenScene {
  schema: number;
  /**
   * Metric version the stored positions were derived under. A mismatch means
   * the geometry no longer corresponds to the current distance function.
   */
  metric_version: number;
  districts: DistrictLayout;
  /** entityKey -> pin. */
  pins: Record<string, DistrictRelativePin>;
  /** entityKey -> district ids the user rejected for it. */
  exclusions: Record<string, string[]>;
  /** entityKey -> last settled position. Warm-start seeds for the layout. */
  positions: Record<string, GardenPosition>;
  /** entityKey -> epoch ms the user last interacted with it. */
  visited: Record<string, number>;
}

export function createScene(): GardenScene {
  return {
    schema: GARDEN_SCENE_SCHEMA,
    metric_version: METRIC_VERSION,
    districts: createDistrictLayout(),
    pins: {},
    exclusions: {},
    positions: {},
    visited: {},
  };
}

/**
 * Pin an entity at an absolute point, stored relative to its district origin.
 *
 * `placedAt` drives the anchor facet's decay, so a placement the user never
 * revisits gradually stops bending the metric.
 */
export function pinEntity(
  scene: GardenScene,
  entityKey: string,
  districtId: string,
  absolute: GardenPosition,
  districtOrigin: GardenPosition,
  placedAt: number = Date.now(),
): GardenScene {
  return {
    ...scene,
    pins: {
      ...scene.pins,
      [entityKey]: {
        district_id: districtId,
        dx: absolute.x - districtOrigin.x,
        dy: absolute.y - districtOrigin.y,
        placed_at_ms: placedAt,
      },
    },
    visited: { ...scene.visited, [entityKey]: placedAt },
  };
}

export function unpinEntity(scene: GardenScene, entityKey: string): GardenScene {
  if (!scene.pins[entityKey]) return scene;
  const pins = { ...scene.pins };
  delete pins[entityKey];
  return { ...scene, pins };
}

/**
 * Resolve a pin to an absolute position.
 *
 * Returns null when the entity has moved districts: the stored offset is
 * meaningless against a different origin, and honouring it would place the
 * entity somewhere it does not belong. Callers surface this as a re-place
 * prompt.
 */
export function resolvePin(
  scene: GardenScene,
  entityKey: string,
  currentDistrictId: string,
  districtOrigin: GardenPosition,
): GardenPosition | null {
  const pin = scene.pins[entityKey];
  if (!pin) return null;
  if (pin.district_id !== currentDistrictId) return null;
  return { x: districtOrigin.x + pin.dx, y: districtOrigin.y + pin.dy };
}

/** Pins whose entity has changed district, and are therefore stale. */
export function stalePins(
  scene: GardenScene,
  districtOf: ReadonlyMap<string, string>,
): string[] {
  return Object.entries(scene.pins)
    .filter(([entityKey, pin]) => {
      const district = districtOf.get(entityKey);
      return district !== undefined && district !== pin.district_id;
    })
    .map(([entityKey]) => entityKey)
    .sort();
}

/** Record that the user rejected a district for an entity. */
export function excludeFromDistrict(
  scene: GardenScene,
  entityKey: string,
  districtId: string,
): GardenScene {
  const existing = scene.exclusions[entityKey] ?? [];
  if (existing.includes(districtId)) return scene;
  return {
    ...scene,
    exclusions: { ...scene.exclusions, [entityKey]: [...existing, districtId].sort() },
  };
}

/**
 * Drift stiffness for an entity.
 *
 * Encodes authority as resistance to motion: a pin is immovable, something the
 * user touched in the last hour resists strongly, a settled entity resists
 * mildly, and an entity with no recorded position is free to find its place.
 */
export function driftFor(
  scene: GardenScene,
  entityKey: string,
  now: number = Date.now(),
): number {
  if (scene.pins[entityKey]) return DRIFT_PINNED;
  const visitedAt = scene.visited[entityKey];
  if (visitedAt !== undefined && now - visitedAt <= RECENTLY_VISITED_MS) return DRIFT_VISITED;
  if (scene.positions[entityKey]) return DRIFT_SETTLED;
  return DRIFT_NEW;
}

/**
 * Decayed weights for every `scene_anchor:*` facet the scene implies.
 *
 * Keyed by token so `facetVector` can look them up directly. A placement's
 * influence is capped and decays with age, which is what stops the map
 * ossifying around one afternoon's arrangement.
 */
export function sceneAnchorWeights(
  scene: GardenScene,
  now: number = Date.now(),
): Map<FacetToken, number> {
  const weights = new Map<FacetToken, number>();
  for (const pin of Object.values(scene.pins)) {
    const token = sceneAnchorToken(pin.district_id);
    const weight = sceneFacetWeight(pin.placed_at_ms, now);
    // Several entities pinned to one district share a token; the strongest
    // (most recent) placement sets its weight rather than accumulating, so
    // repeated placement never compounds.
    weights.set(token, Math.max(weights.get(token) ?? 0, weight));
  }
  return weights;
}

/** Anchor district for an entity, if the user pinned it somewhere. */
export function anchoredDistrict(scene: GardenScene, entityKey: string): string | undefined {
  return scene.pins[entityKey]?.district_id;
}

export function recordPositions(
  scene: GardenScene,
  positions: ReadonlyMap<string, GardenPosition>,
): GardenScene {
  const next = { ...scene.positions };
  for (const [key, position] of positions) next[key] = position;
  return { ...scene, positions: next, metric_version: METRIC_VERSION };
}

export function markVisited(
  scene: GardenScene,
  entityKey: string,
  now: number = Date.now(),
): GardenScene {
  return { ...scene, visited: { ...scene.visited, [entityKey]: now } };
}

/**
 * Drop scene state for entities that no longer exist.
 *
 * Pins and exclusions are user intent and are kept — a deleted agent may come
 * back, and silently discarding a placement is worse than carrying a few stale
 * keys. Positions and visit timestamps are derived and are pruned.
 */
export function pruneScene(scene: GardenScene, liveKeys: ReadonlySet<string>): GardenScene {
  const positions: Record<string, GardenPosition> = {};
  for (const [key, position] of Object.entries(scene.positions)) {
    if (liveKeys.has(key)) positions[key] = position;
  }
  const visited: Record<string, number> = {};
  for (const [key, at] of Object.entries(scene.visited)) {
    if (liveKeys.has(key)) visited[key] = at;
  }
  return { ...scene, positions, visited };
}

/**
 * Whether two scenes are close enough that adopting the newer one would change
 * nothing a user could see.
 *
 * Needed because a layout pass always returns a *new* scene object, while
 * stress majorization converges to a tolerance rather than an exact fixed point
 * — so identity comparison never settles and a write-back loop would spin
 * forever. Comparing content with a world-unit epsilon terminates after the few
 * passes it takes drift to decay below one pixel.
 *
 * `visited` is ignored: it is set by user interaction, not by layout, and
 * should never by itself trigger a re-layout.
 */
export function scenesConverged(
  a: GardenScene,
  b: GardenScene,
  epsilon = 0.5,
): boolean {
  if (!shallowEqualNumbers(a.districts.cells, b.districts.cells)) return false;
  if (!shallowEqualNumbers(a.districts.tombstones, b.districts.tombstones)) return false;
  if (JSON.stringify(a.pins) !== JSON.stringify(b.pins)) return false;
  if (JSON.stringify(a.exclusions) !== JSON.stringify(b.exclusions)) return false;

  const keys = new Set([...Object.keys(a.positions), ...Object.keys(b.positions)]);
  for (const key of keys) {
    const left = a.positions[key];
    const right = b.positions[key];
    if (!left || !right) return false;
    if (Math.abs(left.x - right.x) > epsilon || Math.abs(left.y - right.y) > epsilon) return false;
  }
  return true;
}

function shallowEqualNumbers(
  a: Record<string, number>,
  b: Record<string, number>,
): boolean {
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const key of keys) if (a[key] !== b[key]) return false;
  return true;
}

export interface SceneCompatibility {
  scene: GardenScene;
  /**
   * True when stored geometry predates the current metric. The caller should
   * offer re-derivation with a preview rather than reflowing silently: a silent
   * reflow destroys the user's learned sense of where things live, which is the
   * one thing the map exists to build.
   */
  needsRederive: boolean;
}

/**
 * Load a persisted scene, tolerating absent, malformed, or outdated payloads.
 *
 * Never throws. A corrupt scene degrades to a fresh one, matching how the
 * backend treats a corrupt `topology.json`: layout is recoverable state, and
 * failing to open the map is worse than losing an arrangement.
 */
export function reviveScene(raw: unknown): SceneCompatibility {
  const fresh = createScene();
  if (!raw || typeof raw !== "object") return { scene: fresh, needsRederive: false };

  const candidate = raw as Partial<GardenScene>;
  if (candidate.schema !== GARDEN_SCENE_SCHEMA) {
    return { scene: fresh, needsRederive: false };
  }

  const scene: GardenScene = {
    schema: GARDEN_SCENE_SCHEMA,
    metric_version:
      typeof candidate.metric_version === "number" ? candidate.metric_version : METRIC_VERSION,
    districts: reviveDistricts(candidate.districts),
    pins: reviveRecord(candidate.pins, isPin),
    exclusions: reviveRecord(candidate.exclusions, isStringArray),
    positions: reviveRecord(candidate.positions, isPosition),
    visited: reviveRecord(candidate.visited, (value): value is number => typeof value === "number"),
  };

  // Pins survive a metric change by construction: they are district-relative
  // offsets, not coordinates derived from the metric.
  return { scene, needsRederive: scene.metric_version !== METRIC_VERSION };
}

function reviveDistricts(raw: unknown): DistrictLayout {
  const fallback = createDistrictLayout();
  if (!raw || typeof raw !== "object") return fallback;
  const candidate = raw as Partial<DistrictLayout>;
  return {
    order: typeof candidate.order === "number" ? candidate.order : fallback.order,
    cells: reviveRecord(candidate.cells, (value): value is number => typeof value === "number"),
    tombstones: reviveRecord(
      candidate.tombstones,
      (value): value is number => typeof value === "number",
    ),
  };
}

function reviveRecord<T>(
  raw: unknown,
  guard: (value: unknown) => value is T,
): Record<string, T> {
  if (!raw || typeof raw !== "object") return {};
  const result: Record<string, T> = {};
  for (const [key, value] of Object.entries(raw as Record<string, unknown>)) {
    if (guard(value)) result[key] = value;
  }
  return result;
}

function isPosition(value: unknown): value is GardenPosition {
  return (
    !!value &&
    typeof value === "object" &&
    Number.isFinite((value as GardenPosition).x) &&
    Number.isFinite((value as GardenPosition).y)
  );
}

function isPin(value: unknown): value is DistrictRelativePin {
  if (!value || typeof value !== "object") return false;
  const pin = value as DistrictRelativePin;
  return (
    typeof pin.district_id === "string" &&
    Number.isFinite(pin.dx) &&
    Number.isFinite(pin.dy) &&
    Number.isFinite(pin.placed_at_ms)
  );
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((entry) => typeof entry === "string");
}

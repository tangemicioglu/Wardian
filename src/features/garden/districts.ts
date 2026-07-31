/**
 * Districts: the map layer that makes the Garden learnable.
 *
 * ## Why partition rather than cluster
 *
 * Clustering is unstable — one new agent can re-partition everything, and a map
 * that reorganizes itself cannot be learned. Districts are keyed by a
 * canonical, human-nameable record instead, in priority order:
 *
 *   team -> workspace-fallback group -> worktree -> workspace path -> commons
 *
 * The middle tier is free: `TopologySnapshot.fallback_groups` already groups
 * agents whose neighbours come only from workspace-fallback, and it is
 * documented as currently unconsumed by the frontend. That is exactly the hard
 * case — agents that cohere but have declared no team.
 *
 * ## Why a Hilbert curve
 *
 * District cells are placed along a Hilbert curve rather than in row-major
 * order because it preserves locality in both directions: grid neighbours are
 * curve neighbours, so "near on the map" and "near in the metric" stay
 * correlated as the map grows, and placing a new district near a similar one is
 * a search along the curve.
 *
 * ## Why cells are sticky
 *
 * Once a district owns a cell it keeps it, recorded in the scene. Insertion
 * becomes purely additive: the map grows at its edge and the interior never
 * moves. Emptied districts keep their cell under a TTL tombstone — otherwise
 * removing the last agent from a team and re-adding it would relocate the whole
 * district for no reason a user can perceive.
 *
 * Districts are also a computational firewall. They cap `n` in every
 * superlinear stage of the layout, which is what keeps insertion cost bounded
 * to one district rather than the whole map.
 */

import type { AgentConfig } from "../../types";
import { normalizeEntityPath } from "./entityRef";

/** Districts above this size should be split into parcels, not enlarged. */
export const MAX_DISTRICT_MEMBERS = 60;

/** Grid order: side length is 2^order, so order 5 gives 1024 cells. */
export const DEFAULT_HILBERT_ORDER = 5;

/** How long an emptied district keeps its cell reserved (~14 days). */
export const DISTRICT_TOMBSTONE_TTL_MS = 14 * 24 * 60 * 60 * 1000;

/**
 * Cells examined past the last occupied one when placing a new district. Bounds
 * the search so a sparse map never scans the full 2^(2*order) grid.
 */
const FREE_CELL_SEARCH_WINDOW = 256;

export type DistrictTier = "team" | "fallback" | "worktree" | "workspace" | "commons";

export interface DistrictKey {
  tier: DistrictTier;
  id: string;
}

export function districtId(key: DistrictKey): string {
  return `${key.tier}:${key.id}`;
}

/**
 * The district every entity falls back to. Named "commons" rather than
 * "unaffiliated" because it is a real place on the map — where unassigned
 * blueprints and folders with no owner live — not an error state.
 */
export const COMMONS_DISTRICT_ID = districtId({ tier: "commons", id: "shared" });

export interface AgentDistrictContext {
  /** Team ids from `watchlists/index.json`, in file order. */
  teamIds?: readonly string[];
  /** Index from agent id to `fallback_groups` group key, if any. */
  fallbackGroupId?: string;
  /** `AgentWorktreeSummary.id` — a real entity, preferred over a raw path. */
  worktreeId?: string;
}

/**
 * Resolve an agent's district.
 *
 * Ties inside the team tier are broken by sorting rather than by file order, so
 * an agent in two teams lands in the same district on every machine. Depending
 * on `watchlists/index.json` ordering would make the map machine-specific.
 */
export function resolveAgentDistrict(
  agent: AgentConfig,
  context: AgentDistrictContext = {},
): DistrictKey {
  const teamIds = [...(context.teamIds ?? [])].sort();
  if (teamIds.length > 0) return { tier: "team", id: teamIds[0] };
  if (context.fallbackGroupId) return { tier: "fallback", id: context.fallbackGroupId };
  if (context.worktreeId) return { tier: "worktree", id: context.worktreeId };

  // "Same project": a worktree agent traces back to the repository its worktree
  // came from, matching the existing `shared_workspace` lens semantics.
  const workspace =
    normalizeEntityPath(agent.git_worktree_source) ?? normalizeEntityPath(agent.folder);
  if (workspace) return { tier: "workspace", id: workspace };
  return { tier: "commons", id: "shared" };
}

// --- Hilbert curve --------------------------------------------------------

export interface GridCell {
  x: number;
  y: number;
}

function rotate(n: number, cell: GridCell, rx: number, ry: number): GridCell {
  let { x, y } = cell;
  if (ry === 0) {
    if (rx === 1) {
      x = n - 1 - x;
      y = n - 1 - y;
    }
    return { x: y, y: x };
  }
  return { x, y };
}

/** Curve index to grid cell. */
export function hilbertToCell(index: number, order = DEFAULT_HILBERT_ORDER): GridCell {
  const side = 1 << order;
  let cell: GridCell = { x: 0, y: 0 };
  let remaining = Math.max(0, Math.floor(index));
  for (let step = 1; step < side; step *= 2) {
    const rx = 1 & (remaining >> 1);
    const ry = 1 & (remaining ^ rx);
    cell = rotate(step, cell, rx, ry);
    cell = { x: cell.x + step * rx, y: cell.y + step * ry };
    remaining = Math.floor(remaining / 4);
  }
  return cell;
}

/** Grid cell to curve index. */
export function cellToHilbert(cell: GridCell, order = DEFAULT_HILBERT_ORDER): number {
  const side = 1 << order;
  let { x, y } = cell;
  let index = 0;
  for (let step = side / 2; step > 0; step = Math.floor(step / 2)) {
    const rx = (x & step) > 0 ? 1 : 0;
    const ry = (y & step) > 0 ? 1 : 0;
    index += step * step * ((3 * rx) ^ ry);
    const rotated = rotate(side, { x, y }, rx, ry);
    x = rotated.x;
    y = rotated.y;
  }
  return index;
}

export function hilbertCellCount(order = DEFAULT_HILBERT_ORDER): number {
  return 1 << (2 * order);
}

/**
 * Grid (Chebyshev) distance between two curve indices.
 *
 * Grid distance rather than curve-index difference: the curve preserves
 * locality but is not isometric, so two adjacent cells can sit far apart in
 * index space. Placement cares where things end up on screen.
 */
export function cellDistance(a: number, b: number, order = DEFAULT_HILBERT_ORDER): number {
  const cellA = hilbertToCell(a, order);
  const cellB = hilbertToCell(b, order);
  return Math.max(Math.abs(cellA.x - cellB.x), Math.abs(cellA.y - cellB.y));
}

/** World-space centre of a district cell. */
export function districtCenter(
  index: number,
  options: { order?: number; spacing?: number; origin?: { x: number; y: number } } = {},
): { x: number; y: number } {
  const order = options.order ?? DEFAULT_HILBERT_ORDER;
  const spacing = options.spacing ?? 640;
  const origin = options.origin ?? { x: 0, y: 0 };
  const cell = hilbertToCell(index, order);
  return { x: origin.x + cell.x * spacing, y: origin.y + cell.y * spacing };
}

// --- Sticky assignment ----------------------------------------------------

export interface DistrictLayout {
  order: number;
  /** districtId -> curve index. Persisted in the scene; never reassigned. */
  cells: Record<string, number>;
  /** districtId -> epoch ms at which its reserved cell may be reclaimed. */
  tombstones: Record<string, number>;
}

export function createDistrictLayout(order = DEFAULT_HILBERT_ORDER): DistrictLayout {
  return { order, cells: {}, tombstones: {} };
}

/**
 * Similarity between two districts in [0, 1], 1 = identical. Supplied by the
 * caller (computed from district centroids via the metric) so this module stays
 * independent of facet internals.
 */
export type DistrictSimilarity = (a: string, b: string) => number;

export interface PlaceDistrictsResult {
  layout: DistrictLayout;
  /** Districts that received a cell in this call. */
  placed: string[];
  /** True when no existing district changed cell — the stability invariant. */
  stable: boolean;
}

/**
 * Assign cells to any districts that lack one, leaving existing cells untouched.
 *
 * A new district takes the free cell minimizing its similarity-weighted grid
 * distance to already-placed districts, so it lands beside the districts it
 * most resembles:
 *
 *   cost(c) = sum over placed d of  similarity(new, d) * cellDistance(c, cell(d))
 *
 * Ties break on the lower cell index, keeping the result deterministic.
 */
export function placeDistricts(
  layout: DistrictLayout,
  activeDistrictIds: readonly string[],
  similarity: DistrictSimilarity = () => 0,
): PlaceDistrictsResult {
  const order = layout.order;
  const cells = { ...layout.cells };
  const tombstones = { ...layout.tombstones };
  const before = new Map(Object.entries(cells));
  const placed: string[] = [];
  const active = [...new Set(activeDistrictIds)].sort();

  // A district that comes back is un-tombstoned rather than re-placed: its cell
  // was never removed from `cells`, so it is already absent from `pending` and
  // lands exactly where it was.
  for (const districtIdentifier of active) delete tombstones[districtIdentifier];

  const occupied = new Set<number>(Object.values(cells));
  // Sorted so placement order — and therefore the resulting map — does not
  // depend on how the caller enumerated districts.
  const pending = active.filter((id) => cells[id] === undefined);

  for (const districtIdentifier of pending) {
    const placedIds = Object.keys(cells).sort();
    const cell = chooseFreeCell(
      districtIdentifier,
      placedIds.map((id) => ({ id, cell: cells[id] })),
      occupied,
      similarity,
      order,
    );
    if (cell === null) continue; // Grid exhausted; caller may raise the order.
    cells[districtIdentifier] = cell;
    occupied.add(cell);
    placed.push(districtIdentifier);
  }

  let stable = true;
  for (const [id, cell] of before) {
    if (cells[id] !== cell) stable = false;
  }
  return { layout: { order, cells, tombstones }, placed, stable };
}

function chooseFreeCell(
  districtIdentifier: string,
  placed: ReadonlyArray<{ id: string; cell: number }>,
  occupied: ReadonlySet<number>,
  similarity: DistrictSimilarity,
  order: number,
): number | null {
  const total = hilbertCellCount(order);
  if (placed.length === 0) {
    for (let candidate = 0; candidate < total; candidate += 1) {
      if (!occupied.has(candidate)) return candidate;
    }
    return null;
  }

  const highestOccupied = Math.max(...placed.map((entry) => entry.cell));
  const limit = Math.min(total, highestOccupied + 1 + FREE_CELL_SEARCH_WINDOW);

  let best: number | null = null;
  let bestCost = Infinity;
  for (let candidate = 0; candidate < limit; candidate += 1) {
    if (occupied.has(candidate)) continue;
    let cost = 0;
    for (const entry of placed) {
      const weight = similarity(districtIdentifier, entry.id);
      if (weight <= 0) continue;
      cost += weight * cellDistance(candidate, entry.cell, order);
    }
    if (cost < bestCost) {
      bestCost = cost;
      best = candidate;
    }
  }
  if (best !== null) return best;
  // Every candidate scored equally (no similarity data): take the first free.
  for (let candidate = 0; candidate < total; candidate += 1) {
    if (!occupied.has(candidate)) return candidate;
  }
  return null;
}

/**
 * Tombstone districts that are no longer active, reserving their cells.
 *
 * Called after `placeDistricts` on every pass. Cells are only reclaimed once the
 * TTL expires, so a district that disappears and returns within the window
 * lands exactly where it was.
 */
export function retireDistricts(
  layout: DistrictLayout,
  activeDistrictIds: readonly string[],
  now: number = Date.now(),
  ttlMs: number = DISTRICT_TOMBSTONE_TTL_MS,
): DistrictLayout {
  const active = new Set(activeDistrictIds);
  const cells = { ...layout.cells };
  const tombstones = { ...layout.tombstones };

  for (const districtIdentifier of Object.keys(cells)) {
    if (active.has(districtIdentifier)) {
      delete tombstones[districtIdentifier];
      continue;
    }
    if (tombstones[districtIdentifier] === undefined) {
      tombstones[districtIdentifier] = now + ttlMs;
    }
  }
  for (const [districtIdentifier, expiresAt] of Object.entries(tombstones)) {
    if (expiresAt <= now) {
      delete tombstones[districtIdentifier];
      delete cells[districtIdentifier];
    }
  }
  return { order: layout.order, cells, tombstones };
}

// --- Non-agent membership -------------------------------------------------

/**
 * Explicit facet links that place a non-agent entity into an agent's district.
 *
 * Deliberately explicit rather than nearest-neighbour: a library skill belongs
 * where it is *deployed*, an artifact belongs where it was *produced*. Those
 * are canonical records, so the placement is defensible and O(1). Entities with
 * no such link land in the commons rather than being guessed at.
 */
const DISTRICT_LINK_PREFIXES = ["deployed:agent:", "origin:agent:"] as const;

export function resolveEntityDistrict(
  tokens: readonly string[],
  districtByAgentId: ReadonlyMap<string, string>,
): string {
  const candidates: string[] = [];
  for (const token of tokens) {
    for (const prefix of DISTRICT_LINK_PREFIXES) {
      if (!token.startsWith(prefix)) continue;
      // Strip the "~copy" suffix that marks a copied rather than junctioned
      // deployment: it is weaker evidence of relatedness but still locates the
      // skill in the same district.
      const agentId = token.slice(prefix.length).replace(/~copy$/, "");
      const district = districtByAgentId.get(agentId);
      if (district) candidates.push(district);
    }
  }
  if (candidates.length === 0) return COMMONS_DISTRICT_ID;
  // Sorted for determinism when an entity links to several districts; the most
  // frequently referenced district wins.
  const counts = new Map<string, number>();
  for (const district of candidates) counts.set(district, (counts.get(district) ?? 0) + 1);
  return [...counts.entries()].sort((l, r) => r[1] - l[1] || l[0].localeCompare(r[0]))[0][0];
}

/**
 * Split an oversized district into deterministic parcels.
 *
 * Districts cap `n` in the layout's superlinear stages, so growth past
 * `MAX_DISTRICT_MEMBERS` must add parcels rather than enlarge the input.
 * Parcels are numbered, and members are assigned in sorted key order so the
 * same member lands in the same parcel across sessions.
 */
export function parcelsFor(
  districtIdentifier: string,
  memberKeys: readonly string[],
  maxMembers = MAX_DISTRICT_MEMBERS,
): Map<string, string[]> {
  const sorted = [...memberKeys].sort();
  if (sorted.length <= maxMembers) return new Map([[districtIdentifier, sorted]]);

  const parcelCount = Math.ceil(sorted.length / maxMembers);
  const perParcel = Math.ceil(sorted.length / parcelCount);
  const parcels = new Map<string, string[]>();
  for (let index = 0; index < parcelCount; index += 1) {
    parcels.set(
      `${districtIdentifier}#${index}`,
      sorted.slice(index * perParcel, (index + 1) * perParcel),
    );
  }
  return parcels;
}

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
 * ## Where districts sit
 *
 * Districts occupy slots on a concentric ring lattice centred on the commons —
 * see `ringLattice.ts` for the geometry and why it replaced a Hilbert-curve
 * grid. This module owns which district gets which slot; the lattice owns where
 * a slot is.
 *
 * A new district takes the free slot that minimizes its similarity-weighted
 * distance to the districts already placed, so semantically close districts end
 * up adjacent and the arrangement is a statement about kinship rather than an
 * enumeration order.
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
import {
  CENTER_SLOT,
  RING_ARRANGEMENT,
  slotDistance,
  slotPoint,
} from "./ringLattice";

/** Districts above this size should be split into parcels, not enlarged. */
export const MAX_DISTRICT_MEMBERS = 60;

/**
 * Baseline world-space gap between district cells.
 *
 * A floor rather than a constant: the layout widens it when a district's own
 * members need more room, so districts cannot bleed into one another. See
 * `spacingFor`.
 */
export const DEFAULT_DISTRICT_SPACING = 720;

/**
 * Spacing is rounded up to a multiple of this, and only shrinks once it is a
 * whole step too wide. Without the quantum a single unit drifting a pixel would
 * restate the spacing every pass and slide the entire map.
 */
export const DISTRICT_SPACING_QUANTUM = 120;

/** Clear space left between the bounding boxes of two neighbouring districts. */
export const DISTRICT_MARGIN = 96;

/**
 * Furthest a unit can plausibly settle from its own district's origin.
 *
 * A district holds at most `MAX_DISTRICT_MEMBERS` units before it splits into
 * parcels, those parcels sit on a ring of `DEFAULT_DISTRICT_SPACING * 0.32`, and
 * stress majorization spreads a parcel over a few multiples of the layout scale.
 * Sixty units of typical footprint pack inside a radius of roughly 350, so this
 * is generous by a factor of three.
 *
 * It exists because a stored position can outlive the frame it was written in:
 * if districting changes, yesterday's coordinate is measured against a different
 * origin today, and the difference is not a memory of anywhere — it is an
 * artefact. Bounding what a warm start may claim keeps that artefact from
 * becoming geometry.
 */
export const MAX_DISTRICT_RADIUS = 1200;

/**
 * Hard ceiling on the grid pitch.
 *
 * `spacingFor` derives the pitch from how much room districts actually need, and
 * that derivation is only as trustworthy as its inputs. A pitch is also
 * self-perpetuating — it is persisted, and it sets the frame the next pass reads
 * stored positions in — so a single bad measurement can compound across
 * sessions. This is the backstop: past it, districts may touch, which is a far
 * smaller failure than a map too large to draw.
 */
export const MAX_DISTRICT_SPACING = 2 * MAX_DISTRICT_RADIUS + DISTRICT_MARGIN;

/** How long an emptied district keeps its cell reserved (~14 days). */
export const DISTRICT_TOMBSTONE_TTL_MS = 14 * 24 * 60 * 60 * 1000;

/**
 * Slots examined past the outermost occupied one when placing a new district.
 * Rings are unbounded, so the search needs a window to stay linear in the number
 * of districts rather than in how far out the map happens to reach.
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

// --- Slot geometry --------------------------------------------------------

/**
 * Distance between two slots, in pitches.
 *
 * Re-exported from the lattice so callers reason about slots without depending
 * on how a slot maps to a point.
 */
export function cellDistance(a: number, b: number): number {
  return slotDistance(a, b);
}

/** World-space centre of a district's slot. */
export function districtCenter(
  index: number,
  options: { spacing?: number; origin?: { x: number; y: number } } = {},
): { x: number; y: number } {
  const spacing = options.spacing ?? DEFAULT_DISTRICT_SPACING;
  const origin = options.origin ?? { x: 0, y: 0 };
  const point = slotPoint(index, spacing);
  return { x: origin.x + point.x, y: origin.y + point.y };
}

// --- Sticky assignment ----------------------------------------------------

export interface DistrictLayout {
  /**
   * Slot-to-position mapping the stored cells were assigned under.
   *
   * Persisted, because a slot index only means something under the arrangement
   * that produced it. A mismatch discards the cells rather than reinterpreting
   * them; see `RING_ARRANGEMENT`.
   */
  arrangement: number;
  /** districtId -> slot index. Persisted in the scene; never reassigned. */
  cells: Record<string, number>;
  /** districtId -> epoch ms at which its reserved cell may be reclaimed. */
  tombstones: Record<string, number>;
  /**
   * Current grid pitch, in world units.
   *
   * Persisted because a stored position is absolute: without knowing the pitch
   * that produced it, a later pass cannot recover which district-relative point
   * it represents, and every warm start would be silently offset.
   */
  spacing: number;
}

export function createDistrictLayout(): DistrictLayout {
  return {
    arrangement: RING_ARRANGEMENT,
    cells: {},
    tombstones: {},
    spacing: DEFAULT_DISTRICT_SPACING,
  };
}

/**
 * Grid pitch wide enough for the widest district, with hysteresis.
 *
 * The grid was a fixed pitch while overlap removal ran per district with
 * nothing aware of the cell bounds, so a populous district simply grew past its
 * cell and overlapped its neighbours — the map showed one crowd where the data
 * had two. Deriving the pitch from the measured extents makes the separation a
 * property of the layout rather than a hope about how big districts get.
 *
 * `required` is the largest district's full width or height. Growth is
 * immediate; shrinking waits until a whole quantum has been freed, so a
 * district losing one member does not drag every other district inward.
 *
 * The result is capped at `MAX_DISTRICT_SPACING`, and a `current` above the cap
 * is never treated as a reason to stay there — that is what lets a scene
 * persisted with a runaway pitch recover on its next pass instead of carrying
 * the damage forward forever.
 */
export function spacingFor(required: number, current = DEFAULT_DISTRICT_SPACING): number {
  const target = Math.max(DEFAULT_DISTRICT_SPACING, required + DISTRICT_MARGIN);
  const quantized = Math.min(
    MAX_DISTRICT_SPACING,
    Math.ceil(target / DISTRICT_SPACING_QUANTUM) * DISTRICT_SPACING_QUANTUM,
  );
  if (current > MAX_DISTRICT_SPACING) return quantized;
  if (quantized > current) return quantized;
  if (quantized <= current - DISTRICT_SPACING_QUANTUM) return quantized;
  return current;
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
 * Assign slots to any districts that lack one, leaving existing slots untouched.
 *
 * A new district takes the free slot minimizing its similarity-weighted distance
 * to already-placed districts, so it lands beside the districts it most
 * resembles:
 *
 *   cost(s) = sum over placed d of  similarity(new, d) * slotDistance(s, slot(d))
 *
 * Ties break on the lower slot index, which — because slots are numbered from
 * the centre outward — means an otherwise equal district lands as close to the
 * middle as it can. Placement is deterministic.
 *
 * The commons holds the centre slot unconditionally. It is the district every
 * other one is arranged around, and letting it be pushed outward by arrival
 * order would make the arrangement say something false about it.
 */
export function placeDistricts(
  layout: DistrictLayout,
  activeDistrictIds: readonly string[],
  similarity: DistrictSimilarity = () => 0,
): PlaceDistrictsResult {
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

  // The centre is reserved even when the commons is not currently populated:
  // it is a standing place on the map, and a workspace district that happened to
  // be created first should not inherit the middle and then keep it forever.
  if (cells[COMMONS_DISTRICT_ID] === undefined && !occupied.has(CENTER_SLOT)) {
    if (active.includes(COMMONS_DISTRICT_ID)) {
      cells[COMMONS_DISTRICT_ID] = CENTER_SLOT;
      placed.push(COMMONS_DISTRICT_ID);
    }
    occupied.add(CENTER_SLOT);
  }

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
    );
    cells[districtIdentifier] = cell;
    occupied.add(cell);
    placed.push(districtIdentifier);
  }

  let stable = true;
  for (const [id, cell] of before) {
    if (cells[id] !== cell) stable = false;
  }
  return {
    layout: { arrangement: RING_ARRANGEMENT, cells, tombstones, spacing: layout.spacing },
    placed,
    stable,
  };
}

/**
 * The innermost free slot, or the one a similar district pulls this one toward.
 *
 * Rings are unbounded, so unlike the old fixed grid there is no exhaustion case
 * and no fallback that parks a district on top of the commons. The search is
 * still windowed: scanning past the outermost occupied slot by a bounded margin
 * is enough to find a good seat, and keeps placement linear in the number of
 * districts rather than in the size of the map.
 */
function chooseFreeCell(
  districtIdentifier: string,
  placed: ReadonlyArray<{ id: string; cell: number }>,
  occupied: ReadonlySet<number>,
  similarity: DistrictSimilarity,
): number {
  const firstFree = () => {
    for (let candidate = 0; ; candidate += 1) {
      if (!occupied.has(candidate)) return candidate;
    }
  };
  if (placed.length === 0) return firstFree();

  const highestOccupied = Math.max(...placed.map((entry) => entry.cell));
  const limit = highestOccupied + 1 + FREE_CELL_SEARCH_WINDOW;

  let best: number | null = null;
  let bestCost = Infinity;
  for (let candidate = 0; candidate < limit; candidate += 1) {
    if (occupied.has(candidate)) continue;
    let cost = 0;
    for (const entry of placed) {
      const weight = similarity(districtIdentifier, entry.id);
      if (weight <= 0) continue;
      cost += weight * cellDistance(candidate, entry.cell);
    }
    if (cost < bestCost) {
      bestCost = cost;
      best = candidate;
    }
  }
  // Every candidate scored equally (no similarity data): take the innermost free
  // slot, so an unrelated district fills the map from the centre outward rather
  // than trailing off into a ring of its own.
  return best ?? firstFree();
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
  return { arrangement: layout.arrangement, cells, tombstones, spacing: layout.spacing };
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

/**
 * Where the agents carrying each facet live.
 *
 * Built from agents only, which is what makes it usable as evidence: a token
 * appearing here is one that some agent actually has, so an entity sharing it
 * has a real tie to a populated place rather than to its own kind. It is also
 * why `section:workflows` and friends cannot vote — no agent carries them.
 */
export interface DistrictAffinity {
  /** token -> districtId -> number of agents there carrying it. */
  votes: Map<string, Map<string, number>>;
  /** Corpus size for the IDF weighting: the number of agents indexed. */
  agentCount: number;
}

/**
 * Minimum affinity score before an entity is placed by evidence rather than
 * parked in the commons.
 *
 * Expressed as the IDF of a facet held by *half* the roster, so the admission
 * rule reads: the shared facet must be rarer than "half of everyone", and
 * concentrated enough in one district to keep its weight after the share
 * factor. Below that the placement would be a guess dressed up as a derivation,
 * and the commons is the honest answer.
 *
 * Relative rather than absolute because the score is an IDF, and IDF grows with
 * corpus size: a fixed floor silently means "a third of the roster" on a large
 * one and "nothing qualifies" on a small one. That is exactly what went wrong
 * on the first attempt, where a four-agent fixture placed nothing while the
 * same relationship on a real roster placed cleanly.
 */
export function minimumAffinityScore(agentCount: number): number {
  if (agentCount <= 0) return Infinity;
  return Math.log((agentCount + 1) / (agentCount / 2 + 1));
}

export function buildDistrictAffinity(
  agents: ReadonlyArray<{ tokens: readonly string[]; districtId: string }>,
): DistrictAffinity {
  const votes = new Map<string, Map<string, number>>();
  for (const agent of agents) {
    for (const token of agent.tokens) {
      let byDistrict = votes.get(token);
      if (!byDistrict) {
        byDistrict = new Map();
        votes.set(token, byDistrict);
      }
      byDistrict.set(agent.districtId, (byDistrict.get(agent.districtId) ?? 0) + 1);
    }
  }
  return { votes, agentCount: agents.length };
}

/**
 * District of the agents an entity most resembles, or null when the evidence is
 * too thin to be worth acting on.
 *
 * Scored as an IDF-weighted vote, using the same smoothed statistic as the
 * metric, so rarity does the discriminating without a hand-tuned rule. A
 * workflow whose shell node runs in `D:/Trading/trident` shares that path facet
 * with the two agents living there: `ln(54/3) ≈ 2.9`, decisive. The same
 * workflow also shares `path:d:/` with everyone, where `df == N` makes the IDF
 * exactly 0 and the token free. Nothing needs to know that a drive root is
 * uninteresting and a project directory is not.
 *
 * The share factor keeps a token split across districts from counting fully for
 * each of them, so a facet has to be *concentrated* to decide anything.
 */
export function resolveDistrictByAffinity(
  tokens: readonly string[],
  affinity: DistrictAffinity,
  minimumScore = minimumAffinityScore(affinity.agentCount),
): string | null {
  if (affinity.agentCount === 0) return null;
  const scores = new Map<string, number>();

  for (const token of tokens) {
    const byDistrict = affinity.votes.get(token);
    if (!byDistrict) continue;
    let holders = 0;
    for (const count of byDistrict.values()) holders += count;
    const weight = Math.log((affinity.agentCount + 1) / (holders + 1));
    if (weight <= 0) continue;
    for (const [district, count] of byDistrict) {
      scores.set(district, (scores.get(district) ?? 0) + (weight * count) / holders);
    }
  }

  let best: string | null = null;
  let bestScore = 0;
  // Sorted for determinism when two districts score identically.
  for (const [district, score] of [...scores].sort((l, r) => l[0].localeCompare(r[0]))) {
    if (score > bestScore) {
      best = district;
      bestScore = score;
    }
  }
  return bestScore >= minimumScore ? best : null;
}

export function resolveEntityDistrict(
  tokens: readonly string[],
  districtByAgentId: ReadonlyMap<string, string>,
  /** Consulted only when no explicit link resolves; see `resolveDistrictByAffinity`. */
  affinity?: DistrictAffinity,
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
  if (candidates.length === 0) {
    return (affinity && resolveDistrictByAffinity(tokens, affinity)) ?? COMMONS_DISTRICT_ID;
  }
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

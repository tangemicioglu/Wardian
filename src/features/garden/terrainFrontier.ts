/**
 * The level-of-detail rule for Garden terrain — which is also its ingestion
 * boundary.
 *
 * `facets.ts` already commits to the principle: files are not corpus members
 * until a folder is expanded, and expansion increments `df` along the affected
 * ancestor chain only. This module makes the same boundary govern drawing.
 * Wardian has no recursive filesystem enumeration in either language, and does
 * not need one — a folder is listed when, and only when, its cell is large
 * enough on screen to be worth subdividing.
 *
 * Two budgets bound the result. Both are stated in the spec and enforced here
 * rather than hoped for, because the failure they prevent is not a slow map but
 * an unbounded one: a single `node_modules` expansion is thousands of listings.
 */

import { area, type TerrainCell, type TerrainListing, type TerrainRect } from "./terrain";

/** Screen area, in CSS pixels, at which a folder is worth listing. */
export const EXPAND_AREA_PX = 5200;

/**
 * Screen area below which a cell is not subdivided at all.
 *
 * Deliberately smaller than `EXPAND_AREA_PX`, so a listing already in hand keeps
 * drawing slightly past the point where a new one would be fetched. Equal
 * thresholds would make a cell hovering at the boundary fetch, draw, drop, and
 * fetch again on a one-pixel zoom change.
 */
export const SUBDIVIDE_AREA_PX = 2600;

/**
 * Cell budget offered to the districts, protecting scene-graph size.
 *
 * **Not a hard ceiling on drawn cells**, and the distinction is load-bearing
 * enough to state here rather than leave to be discovered. `districtCellBudget`
 * divides this between the districts and then floors each share at
 * `MIN_DISTRICT_CELLS`, because a share below that cannot admit even one
 * directory level and a district that can only draw its own root cell is not
 * showing terrain at all. On a roster with many districts the floor wins, and
 * the drawn total is `districts * MIN_DISTRICT_CELLS`.
 *
 * The alternative was a global cap with proportional shares, and it is worse:
 * 2000 across 37 districts is 54 cells each, which is under one level
 * everywhere, so every district on a busy roster would show a single flat plot.
 * A budget that is honoured by making the feature useless is not a budget worth
 * honouring — so the ceiling gives way and the floor holds.
 *
 * What actually bounds the scene is per-district: the frontier only expands
 * cells above `EXPAND_AREA_PX` of *screen* area and only inside the viewport, so
 * cell count tracks what is visible rather than what exists.
 */
export const MAX_TERRAIN_CELLS = 2000;

/** Hard ceiling on cached listings, protecting memory and calls in flight. */
export const MAX_FRONTIER_DIRS = 400;

/** Listings requested per frontier evaluation. */
export const MAX_LISTING_REQUESTS = 32;

export interface TerrainViewport {
  /** World-space rectangle currently visible. */
  world: TerrainRect;
  /** World-to-screen factor. */
  scale: number;
}

/**
 * World-space area below which `buildTerrain` stops subdividing.
 *
 * Screen area scales with the square of the zoom factor, so the world-space
 * threshold is the screen threshold divided by `scale^2`.
 */
export function minSubdivideArea(scale: number): number {
  if (!Number.isFinite(scale) || scale <= 0) return Infinity;
  return SUBDIVIDE_AREA_PX / (scale * scale);
}

export function intersectsViewport(rect: TerrainRect, world: TerrainRect): boolean {
  return (
    rect.x < world.x + world.width &&
    rect.x + rect.width > world.x &&
    rect.y < world.y + world.height &&
    rect.y + rect.height > world.y
  );
}

/**
 * Directories worth listing next.
 *
 * Only truncated directory cells qualify — a cell whose children are already
 * drawn needs nothing — and only those the user can actually see. Sorted by
 * descending screen area so the budget spends itself on the folders the user is
 * looking at rather than on whichever happened to be enumerated first.
 */
export function frontierRequests(
  cells: readonly TerrainCell[],
  viewport: TerrainViewport,
  listings: ReadonlyMap<string, TerrainListing>,
  budget: {
    maxRequests?: number;
    maxFrontierDirs?: number;
    /**
     * Directories already requested and not yet cached.
     *
     * Counted against the budget because the cache is what the budget bounds,
     * and an in-flight request is a cache entry that has not landed yet. Without
     * it, two waves of the expansion pass could each measure the same
     * `listings.size` and together overshoot by a whole `maxRequests`.
     */
    inFlight?: number;
  } = {},
): string[] {
  const maxRequests = budget.maxRequests ?? MAX_LISTING_REQUESTS;
  const maxFrontierDirs = budget.maxFrontierDirs ?? MAX_FRONTIER_DIRS;
  if (maxRequests <= 0) return [];

  const remaining = maxFrontierDirs - listings.size - Math.max(0, budget.inFlight ?? 0);
  if (remaining <= 0) return [];

  const scaleArea = viewport.scale * viewport.scale;
  const candidates: Array<{ path: string; screenArea: number }> = [];
  const seen = new Set<string>();

  for (const cell of cells) {
    if (!cell.isDir || !cell.truncated) continue;
    if (listings.has(cell.path) || seen.has(cell.path)) continue;
    if (!intersectsViewport(cell.rect, viewport.world)) continue;
    const screenArea = area(cell.rect) * scaleArea;
    if (screenArea < EXPAND_AREA_PX) continue;
    seen.add(cell.path);
    candidates.push({ path: cell.path, screenArea });
  }

  candidates.sort(
    (left, right) => right.screenArea - left.screenArea || left.path.localeCompare(right.path),
  );
  return candidates.slice(0, Math.min(maxRequests, remaining)).map((entry) => entry.path);
}

/**
 * Cached listings a filesystem event has made stale.
 *
 * Returns paths to *re-fetch*, never a map to install. Evicting and refetching
 * is what made the ground blink: dropping a subtree collapses the district to
 * its root cell for a debounce plus a round trip, and an agent writing steadily
 * keeps it collapsed. Refreshing in place costs one stale render instead — a
 * directory deleted a moment ago survives until its parent's listing lands,
 * which is a far smaller lie than a district that keeps vanishing.
 *
 * Scoping is by *parent*, because a directory listing is what asserts a child
 * exists: refreshing the parent of a changed path both adds new children and
 * removes deleted ones, and a deleted directory's own stale listing is orphaned
 * rather than drawn. `changed_paths` carries every path the watcher saw in the
 * debounce window (it accumulates into a `BTreeSet` rather than sampling), so
 * this is not the guess a per-event payload would have been. An empty set falls
 * back to every cached listing under the root.
 */
export function staleListings(
  listings: ReadonlyMap<string, TerrainListing>,
  root: string,
  changedPaths: readonly string[],
  budget: { maxRequests?: number } = {},
): string[] {
  const maxRequests = budget.maxRequests ?? MAX_LISTING_REQUESTS;
  if (maxRequests <= 0 || listings.size === 0) return [];
  const prefix = root.endsWith("/") ? root : `${root}/`;
  const under = (path: string) => path === root || path.startsWith(prefix);

  const stale = new Set<string>();
  if (changedPaths.length === 0) {
    for (const path of listings.keys()) {
      if (under(path)) stale.add(path);
    }
  } else {
    for (const changed of changedPaths) {
      // The path itself, for a watcher that reports the directory rather than
      // the file inside it, and its parent, which is what lists it.
      if (listings.has(changed) && under(changed)) stale.add(changed);
      const separator = changed.lastIndexOf("/");
      if (separator <= 0) continue;
      const parent = changed.slice(0, separator);
      if (listings.has(parent) && under(parent)) stale.add(parent);
    }
  }

  // Shallowest first: a parent's refresh settles whether its children exist at
  // all, so when the budget binds it is spent where it decides the most.
  return [...stale]
    .sort((left, right) => depthOf(left) - depthOf(right) || left.localeCompare(right))
    .slice(0, maxRequests);
}

function depthOf(path: string): number {
  let depth = 0;
  for (let index = 0; index < path.length; index += 1) {
    if (path.charCodeAt(index) === 47) depth += 1;
  }
  return depth;
}

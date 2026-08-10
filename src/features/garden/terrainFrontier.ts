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

/** Hard ceiling on drawn cells, protecting scene-graph size and hit-testing. */
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

function intersectsViewport(rect: TerrainRect, world: TerrainRect): boolean {
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
  } = {},
): string[] {
  const maxRequests = budget.maxRequests ?? MAX_LISTING_REQUESTS;
  const maxFrontierDirs = budget.maxFrontierDirs ?? MAX_FRONTIER_DIRS;
  if (maxRequests <= 0) return [];

  const remaining = maxFrontierDirs - listings.size;
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
 * Drop cached listings under a changed root.
 *
 * `explorer-changed` reports the watched root, not the file, so invalidation is
 * by prefix. Coarser than necessary and deliberately so: the alternative is
 * trusting a debounced watcher to have reported every path in a burst, and a
 * stale listing is a directory the map claims exists after it was deleted.
 */
export function invalidateUnder(
  listings: ReadonlyMap<string, TerrainListing>,
  root: string,
): Map<string, TerrainListing> {
  const next = new Map(listings);
  const prefix = root.endsWith("/") ? root : `${root}/`;
  for (const path of listings.keys()) {
    if (path === root || path.startsWith(prefix)) next.delete(path);
  }
  return next;
}

/**
 * Ground geometry for the Garden map.
 *
 * Districts hold agents and blueprints. This module draws what they act *on*:
 * a deterministic subdivision of the directory subtrees a district's agents
 * work in, rendered as territory beneath the units.
 *
 * ## Why files are not units
 *
 * `docs/specs/2026-07-30-garden-metric-map.md` decides this twice over. An
 * entity that is an attribute of another renders *on* it, and — the binding
 * constraint — districts exist to cap `n` in every superlinear layout stage.
 * This repository alone carries 1466 tracked files against 53 agents, so
 * admitting files as layout entities would make `n` a property of the disk and
 * delete the reason districts were built. Terrain is a subdivision of territory
 * a district already occupies, so it costs the layout exactly nothing.
 *
 * ## What a cell's area means
 *
 * A treemap normally weights cells by a quantity, and none is available:
 * `FileNode` carries `name`, `path`, `is_dir`, and `extension`, and a
 * directory's recursive size cannot be had without the crawl this design exists
 * to avoid. Weighting files by bytes while directories fell back to a constant
 * would make `package-lock.json` the largest thing in a repository and `src/` a
 * small square — not a defensible claim about a codebase.
 *
 * So area is *not* size. It is a share of the parent: a folder's children
 * divide it between them, and a cell's area therefore says how deep in the tree
 * it sits and how many siblings it has. A file directly in the repository root
 * is a peer of `src/` and is drawn as one, which is why a single file can look
 * larger than a folder several levels down.
 *
 * The one weighting the data does support is `is_dir`: a folder holds at least
 * one thing and a file holds none, so folders take `DIR_WEIGHT` shares to a
 * file's one. Both values are known the moment the parent is listed, which is
 * what keeps this admissible — a child's rect stays fixed from its parent's
 * single listing and cannot shift as deeper listings arrive. Weighting a folder
 * by its *subtree* would be more informative and is not available at any price
 * the stability contract can pay: it would make geometry a function of the
 * frontier, so zooming in would resize everything already on screen.
 *
 * ## What geometry may depend on
 *
 * A cell's rect is a function of its parent's rect and its siblings, and of
 * nothing else. Change data never reaches this module — see `terrainPaint.ts`
 * for where it enters. The viewport enters only as `minSubdivideArea`, a scalar
 * that can add or drop a whole level of detail but provably cannot alter a rect
 * that is drawn at both thresholds. `terrain.test.ts` locks that invariant.
 */

import type { GardenPosition } from "./garden.types";

/** World-space rectangle. */
export interface TerrainRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** One directory listing — the unit of ingestion. */
export interface TerrainListing {
  /** Normalized absolute path of the listed directory. */
  path: string;
  children: readonly TerrainChild[];
}

export interface TerrainChild {
  name: string;
  /** Normalized absolute path. */
  path: string;
  isDir: boolean;
  extension: string | null;
}

export interface TerrainDistrict {
  /** Distinct normalized workspace roots of the district's agents. */
  roots: readonly string[];
  /**
   * root -> where that root's agents settled, relative to the district origin.
   *
   * Used to decide *which* cell a root gets, never what the cells look like.
   * Absent for a district whose members have no position yet, which falls back
   * to sorted order.
   */
  anchors?: ReadonlyMap<string, GardenPosition>;
  origin: GardenPosition;
  /**
   * Radius of the ground disc, already resolved by `groundRadiusFor`.
   *
   * Resolved once by the projection rather than by each consumer, because the
   * geometry and the clip must agree exactly: a cell wider than the clip is
   * drawn and then invisibly cut, which reads as a missing folder.
   */
  radius: number;
}

export interface TerrainCell {
  /** Normalized absolute path. Unique across the map, and the cell's key. */
  path: string;
  name: string;
  isDir: boolean;
  districtId: string;
  /** 0 for a root, incrementing with nesting. Drives stroke weight and labels. */
  depth: number;
  rect: TerrainRect;
  /**
   * True when this is a directory whose children are not drawn — either no
   * listing has been fetched, or the budget stopped at this level. Rendered as
   * solid ground with a marker, which reads as "there is more here" rather than
   * asserting the folder is empty.
   */
  truncated: boolean;
}

export interface TerrainInput {
  /** districtId -> roots and extent. */
  districts: ReadonlyMap<string, TerrainDistrict>;
  /** Listings fetched so far, keyed by normalized directory path. */
  listings: ReadonlyMap<string, TerrainListing>;
  /**
   * World-space area below which a cell is not subdivided.
   *
   * Derived from zoom by `minSubdivideArea`. Without it, a cached deep listing
   * would keep drawing hundreds of sub-pixel cells once the user zoomed back
   * out — cost with no information. It gates *whether* a level is drawn, never
   * where a drawn cell sits.
   */
  minSubdivideArea: number;
  /** Ceiling on drawn cells, divided between the districts. */
  maxCells: number;
}

/**
 * Floor on a district's share of `maxCells`.
 *
 * Sized against what one level actually costs. A district holding four
 * repository roots of about forty-six entries each needs ~190 cells to draw its
 * *first* level, and the budget is all-or-nothing per level — so a floor below
 * that does not show less detail, it shows none, and the ground sits there as
 * bare squares that never open. An earlier value of 64 did exactly that.
 *
 * Generous is safe here because the budget is not what bounds the map. The
 * frontier is: a folder is listed only when its cell is large on screen, so
 * districts the user is not looking at contribute one cell each no matter what
 * they are allowed. This is a backstop against one pathological folder, not the
 * mechanism that keeps the scene graph small.
 */
export const MIN_DISTRICT_CELLS = 512;

/**
 * A district's share of the cell budget.
 *
 * The floor can raise the total above `maxCells`: `districtCount` districts each
 * get at least `MIN_DISTRICT_CELLS`, so `maxCells` is what the districts divide,
 * not a ceiling on what they draw. That is deliberate — see `MAX_TERRAIN_CELLS`
 * for why a hard ceiling divided across a busy roster shows nothing at all — and
 * it is stated in both places because a constant named like a limit that is not
 * one will otherwise be read as a bug.
 */
export function districtCellBudget(maxCells: number, districtCount: number): number {
  if (districtCount <= 0) return maxCells;
  // The floor never exceeds the whole budget, so a caller asking for a small
  // ceiling gets one rather than silently getting 64.
  const floor = Math.min(maxCells, MIN_DISTRICT_CELLS);
  return Math.max(floor, Math.floor(maxCells / districtCount));
}

/**
 * Ground radius a district is inflated *towards*.
 *
 * A district holding one agent measures a near-zero extent, and territory that
 * collapses to a point is not territory. This is the smallest plot worth
 * drawing — but it is a target, never a guarantee. See `groundRadiusFor`.
 */
export const MIN_GROUND_RADIUS = 120;

/**
 * Clear space left between two districts' ground discs, per side.
 *
 * This is the single knob for how much grass the map shows. It sets both halves
 * of the separation: the lattice reserves `2 * GROUND_GAP` between neighbouring
 * footprints (`DISTRICT_GROUND_MARGIN`), and `groundRadiusFor` clips a ground to
 * leave exactly that much when a slot ends up tighter than the reservation. The
 * two cannot drift apart because there is only one number.
 *
 * It buys less than it looks like on a roster of small districts. Their step is
 * `2 * MIN_GROUND_RADIUS + 2 * GROUND_GAP`, so the floor dominates and the gap
 * moves the pitch by a few percent. The remaining slack is angular rather than
 * radial: ring `r` holds `6r` slots at a radius set by clearing the ring inside
 * it, which on a map several rings deep is wider than the ring's own contents
 * need. Closing that means deriving slot count from radius, which makes a slot
 * index mean something roster-dependent — a `RING_ARRANGEMENT` break, not a
 * tuning change.
 */
export const GROUND_GAP = 16;

/**
 * Room the ring lattice must reserve for a district.
 *
 * The lattice used to size slots against the *unit extent* alone, and the two
 * were never introduced to each other. A ring of one-agent districts measured
 * extents around 48 and got a radial step of 192, so every ground on it was
 * clipped to 72; a ring sitting outside a large commons was pushed out so far
 * that its grounds stayed at 120 with hundreds of units of grass between them.
 * Same map, both failure modes, opposite directions.
 *
 * Sizing the lattice against what will actually be *drawn* collapses both into
 * one number: with `DISTRICT_GROUND_MARGIN` between footprints, the clear space
 * between two grounds is the same everywhere, and `groundRadiusFor`'s clip stops
 * being what decides how big a ground gets.
 *
 * Applied to districts that draw no ground as well. In practice that is only the
 * commons, whose extent exceeds the floor anyway — and threading "does this
 * district have a workspace" into the lattice would put a terrain concept inside
 * the geometry module for no gain.
 */
export function districtFootprint(extent: number): number {
  const safe = Math.max(0, Number.isFinite(extent) ? extent : 0);
  return Math.max(safe, MIN_GROUND_RADIUS);
}

/**
 * Clear space the lattice leaves between two districts' footprints.
 *
 * One `GROUND_GAP` per side, which is exactly what `groundRadiusFor` promises,
 * so the lattice reserves the separation the ground was already going to take.
 */
export const DISTRICT_GROUND_MARGIN = 2 * GROUND_GAP;

/**
 * Radius of a district's ground.
 *
 * The lattice now reserves `districtFootprint` per district, so the floor
 * normally fits and this returns `MIN_GROUND_RADIUS` outright. The cap remains
 * because the lattice is sized from each *ring's widest* district and a slot can
 * still end up tighter than the reservation implies — and because a district
 * whose units genuinely reach further than its neighbour gap must keep its full
 * extent: at that point the *units* overlap too, and shrinking the ground would
 * hide a layout problem rather than fix one.
 *
 * @param extent Distance from the origin to the district's furthest unit.
 * @param nearestNeighbour Distance to the closest other district's origin,
 *   `Infinity` when this is the only district on the map.
 */
export function groundRadiusFor(extent: number, nearestNeighbour: number): number {
  const safe = Math.max(0, Number.isFinite(extent) ? extent : 0);
  const cap = Math.max(0, nearestNeighbour / 2 - GROUND_GAP);
  return Math.max(safe, Math.min(MIN_GROUND_RADIUS, cap));
}

/**
 * Grid an anchor is snapped to before it decides anything.
 *
 * The root-to-cell assignment is a discrete choice made from continuous
 * positions, so without quantization a unit drifting a pixel could swap two
 * roots' territory and the ground would jump for no visible reason. The same
 * reasoning and the same remedy as `RING_EXTENT_QUANTUM` in the lattice: the
 * ground rearranges only when a district's members move a visible amount.
 */
export const ANCHOR_QUANTUM = 40;

export function quantizeAnchor(point: GardenPosition): GardenPosition {
  return {
    x: Math.round(point.x / ANCHOR_QUANTUM) * ANCHOR_QUANTUM,
    y: Math.round(point.y / ANCHOR_QUANTUM) * ANCHOR_QUANTUM,
  };
}

/**
 * Give each root the cell nearest the agents that work in it.
 *
 * A district spanning several repositories lays its ground out in sorted path
 * order while the metric places its agents by what they resemble, so the two
 * orders agree only by luck. An agent then sits over a neighbour's ground and
 * reads as belonging to a repository it has never touched — which is the one
 * thing territory is supposed to communicate.
 *
 * The ground moves rather than the units, and the direction is forced: ground
 * radius is derived from how far the units settled, so units deriving from cell
 * rects would close a cycle. It is also the safer half to move. Unit positions
 * are persisted, pinned, and dragged by the operator; the ground is derived per
 * session and already reflows whenever a folder's contents change.
 *
 * Only the *assignment* is chosen here. Cells keep the shapes squarify gave
 * them in sorted order, so the set of rects is still a function of the root set
 * and the ground square alone, and every stability property of the treemap
 * survives the permutation.
 */
export function assignRootsToCells(
  roots: readonly string[],
  cells: readonly TerrainRect[],
  anchors: ReadonlyMap<string, GardenPosition> | undefined,
  origin: GardenPosition,
): string[] {
  if (!anchors || cells.length !== roots.length || roots.length < 2) return [...roots];

  const cost = (rootIndex: number, cellIndex: number): number => {
    const anchor = anchors.get(roots[rootIndex]);
    if (!anchor) return 0;
    const rect = cells[cellIndex];
    const dx = origin.x + anchor.x - (rect.x + rect.width / 2);
    const dy = origin.y + anchor.y - (rect.y + rect.height / 2);
    return dx * dx + dy * dy;
  };

  // Exhaustive below the point where it stops being free: six repositories is
  // 720 orderings, evaluated once per layout, and branch-and-bound prunes most
  // of them. Beyond that, a greedy pass over the cheapest pairs.
  if (roots.length <= 6) {
    let best: number[] | null = null;
    let bestCost = Number.POSITIVE_INFINITY;
    const order: number[] = [];
    const used = new Array<boolean>(roots.length).fill(false);
    const walk = (cellIndex: number, running: number) => {
      if (running >= bestCost) return;
      if (cellIndex === cells.length) {
        bestCost = running;
        best = [...order];
        return;
      }
      for (let rootIndex = 0; rootIndex < roots.length; rootIndex += 1) {
        if (used[rootIndex]) continue;
        used[rootIndex] = true;
        order.push(rootIndex);
        walk(cellIndex + 1, running + cost(rootIndex, cellIndex));
        order.pop();
        used[rootIndex] = false;
      }
    };
    walk(0, 0);
    return best ? (best as number[]).map((rootIndex) => roots[rootIndex]) : [...roots];
  }

  const pairs: Array<{ rootIndex: number; cellIndex: number; cost: number }> = [];
  for (let rootIndex = 0; rootIndex < roots.length; rootIndex += 1) {
    for (let cellIndex = 0; cellIndex < cells.length; cellIndex += 1) {
      pairs.push({ rootIndex, cellIndex, cost: cost(rootIndex, cellIndex) });
    }
  }
  // Ties break on the indices, which are sorted-root order, so the result does
  // not depend on how the pairs happened to be enumerated.
  pairs.sort(
    (left, right) =>
      left.cost - right.cost ||
      left.rootIndex - right.rootIndex ||
      left.cellIndex - right.cellIndex,
  );
  const takenRoot = new Set<number>();
  const takenCell = new Set<number>();
  const result = new Array<string | null>(cells.length).fill(null);
  for (const pair of pairs) {
    if (takenRoot.has(pair.rootIndex) || takenCell.has(pair.cellIndex)) continue;
    takenRoot.add(pair.rootIndex);
    takenCell.add(pair.cellIndex);
    result[pair.cellIndex] = roots[pair.rootIndex];
  }
  const leftover = roots.filter((root) => !result.includes(root));
  return result.map((root) => root ?? leftover.shift() ?? roots[0]);
}

/**
 * Shares a folder takes to a file's one.
 *
 * A constant claim — "a folder holds more than a file" — rather than an
 * estimate of how much more. Anything proportional to the real subtree would
 * have to be measured, and measuring is the crawl this design avoids.
 */
export const DIR_WEIGHT = 3;

/** Inset applied inside a folder before its children are laid out. */
const CELL_PADDING = 3;

/**
 * Strip reserved at the top of a folder cell for its label.
 *
 * Taken from the folder's own area rather than drawn over its children, so a
 * label never sits on top of a child cell it does not name.
 */
const CELL_HEADER = 11;

/** Below this, a folder has no room for a label strip and gets none. */
const HEADER_MIN_HEIGHT = 44;

/**
 * Squarified treemap (Bruls, Huizing & van Wijk).
 *
 * Chosen over slice-and-dice because aspect ratio is what makes a small cell
 * clickable and labellable; a slice-and-dice layout of 40 equal children
 * produces 40 slivers.
 *
 * Deterministic: items are placed in the order given, and no tie is broken by
 * anything but that order.
 */
export function squarify<T>(
  items: readonly { value: number; datum: T }[],
  rect: TerrainRect,
): Array<{ datum: T; rect: TerrainRect }> {
  const placed: Array<{ datum: T; rect: TerrainRect }> = [];
  if (items.length === 0 || rect.width <= 0 || rect.height <= 0) return placed;

  const total = items.reduce((sum, item) => sum + Math.max(item.value, 0), 0);
  if (total <= 0) return placed;

  // Work in area units: scale values so the whole set fills the rect exactly.
  const areaScale = (rect.width * rect.height) / total;
  const queue = items.map((item) => ({
    datum: item.datum,
    area: Math.max(item.value, 0) * areaScale,
  }));

  let free: TerrainRect = { ...rect };
  let row: typeof queue = [];
  let rowArea = 0;

  const shortSide = () => Math.min(free.width, free.height);

  /** Worst aspect ratio in `row` if `extra` were added to it. */
  const worst = (extra: number) => {
    const side = shortSide();
    if (side <= 0) return Infinity;
    const areas = extra > 0 ? [...row.map((entry) => entry.area), extra] : row.map((e) => e.area);
    if (areas.length === 0) return Infinity;
    const sum = areas.reduce((total_, area) => total_ + area, 0);
    if (sum <= 0) return Infinity;
    const max = Math.max(...areas);
    const min = Math.min(...areas);
    const side2 = side * side;
    const sum2 = sum * sum;
    return Math.max((side2 * max) / sum2, sum2 / (side2 * min));
  };

  const flushRow = () => {
    if (row.length === 0) return;
    const side = shortSide();
    if (side <= 0) {
      row = [];
      rowArea = 0;
      return;
    }
    const thickness = rowArea / side;
    const horizontal = free.width < free.height;

    let offset = 0;
    for (const entry of row) {
      const length = rowArea > 0 ? (entry.area / rowArea) * side : 0;
      placed.push({
        datum: entry.datum,
        rect: horizontal
          ? { x: free.x + offset, y: free.y, width: length, height: thickness }
          : { x: free.x, y: free.y + offset, width: thickness, height: length },
      });
      offset += length;
    }

    free = horizontal
      ? { x: free.x, y: free.y + thickness, width: free.width, height: free.height - thickness }
      : { x: free.x + thickness, y: free.y, width: free.width - thickness, height: free.height };
    row = [];
    rowArea = 0;
  };

  for (const entry of queue) {
    if (row.length === 0 || worst(entry.area) <= worst(0)) {
      row.push(entry);
      rowArea += entry.area;
      continue;
    }
    flushRow();
    row.push(entry);
    rowArea += entry.area;
  }
  flushRow();

  return placed;
}

/**
 * Build every drawn cell.
 *
 * Level by level, so that when the budget binds the deepest level is dropped
 * *whole*. Dropping a level partially would show half of one folder's children
 * and none of its neighbour's, which reads as a claim about the two folders
 * rather than as a budget.
 *
 * Each district descends against its own share of `maxCells`, and this is a
 * stability requirement rather than a fairness one. A single shared pool makes
 * the deepest level a function of the *total* cell count, so one district
 * receiving a listing silently deletes a level in a district on the other side
 * of the map, and the next invalidation there puts it back. The map blinks in
 * places nothing happened to. A district's detail must depend on that
 * district's data.
 *
 * The share is `districtCellBudget`, which floors well above what one level
 * costs. A share too small to admit a level does not degrade gracefully — the
 * cut is whole-level, so the ground draws as bare squares that never open no
 * matter how far in the user zooms.
 */
export function buildTerrain(input: TerrainInput): TerrainCell[] {
  const cells: TerrainCell[] = [];

  interface Pending {
    cell: TerrainCell;
    /** Rect the children are laid out into, already inset. */
    inner: TerrainRect;
  }

  // Districts in id order so the output is identical run to run.
  const districtIds = [...input.districts.keys()]
    .sort()
    .filter((id) => (input.districts.get(id)?.roots.length ?? 0) > 0);
  const budget = districtCellBudget(input.maxCells, districtIds.length);

  for (const districtId of districtIds) {
    const district = input.districts.get(districtId);
    if (!district) continue;

    const radius = district.radius;
    if (radius <= 0) continue;
    const disc = { origin: district.origin, radius };
    const ground: TerrainRect = {
      x: district.origin.x - radius,
      y: district.origin.y - radius,
      width: radius * 2,
      height: radius * 2,
    };

    const roots = [...new Set(district.roots)].sort();
    // Shapes first, in sorted order, so the set of rects depends only on the
    // root set. Which root receives which of those rects is then chosen to sit
    // under the agents that work in it.
    const shapes = squarify(
      roots.map((path) => ({ value: 1, datum: path })),
      ground,
    ).map((entry) => entry.rect);
    const ordered = assignRootsToCells(roots, shapes, district.anchors, district.origin);
    const placed = shapes.map((rect, index) => ({ datum: ordered[index], rect }));

    let districtCells = 0;
    let frontier: Pending[] = [];

    for (const { datum: path, rect } of placed) {
      if (!intersectsDisc(rect, disc)) continue;
      const cell: TerrainCell = {
        path,
        name: basename(path),
        isDir: true,
        districtId,
        depth: 0,
        rect,
        truncated: true,
      };
      cells.push(cell);
      districtCells += 1;
      frontier.push({ cell, inner: innerRect(rect) });
    }

    // Breadth-first descent. Each pass produces one whole level or none.
    let depth = 1;
    while (frontier.length > 0) {
      const next: Pending[] = [];
      const level: TerrainCell[] = [];
      // Parents that contributed to this level, so they can be un-truncated only
      // if the level is actually admitted.
      const expanded: TerrainCell[] = [];

      for (const parent of frontier) {
        const listing = input.listings.get(parent.cell.path);
        if (!listing || listing.children.length === 0) continue;
        if (area(parent.cell.rect) < input.minSubdivideArea) continue;
        if (parent.inner.width <= 0 || parent.inner.height <= 0) continue;

        const placedChildren = squarify(
          listing.children.map((child) => ({
            value: child.isDir ? DIR_WEIGHT : 1,
            datum: child,
          })),
          parent.inner,
        );
        let contributed = false;
        for (const { datum: child, rect } of placedChildren) {
          if (!intersectsDisc(rect, disc)) continue;
          const cell: TerrainCell = {
            path: child.path,
            name: child.name,
            isDir: child.isDir,
            districtId,
            depth,
            rect,
            truncated: child.isDir,
          };
          level.push(cell);
          contributed = true;
          if (child.isDir) next.push({ cell, inner: innerRect(rect) });
        }
        if (contributed) expanded.push(parent.cell);
      }

      if (level.length === 0) break;
      if (districtCells + level.length > budget) break;

      for (const parent of expanded) parent.truncated = false;
      cells.push(...level);
      districtCells += level.length;
      frontier = next;
      depth += 1;
    }
  }

  return cells;
}

/** Rect a folder's children are laid out into: padded, with a label strip. */
function innerRect(rect: TerrainRect): TerrainRect {
  const pad = Math.min(CELL_PADDING, rect.width / 8, rect.height / 8);
  const header = rect.height >= HEADER_MIN_HEIGHT ? CELL_HEADER : 0;
  return {
    x: rect.x + pad,
    y: rect.y + pad + header,
    width: Math.max(0, rect.width - pad * 2),
    height: Math.max(0, rect.height - pad * 2 - header),
  };
}

export function area(rect: TerrainRect): number {
  return Math.max(0, rect.width) * Math.max(0, rect.height);
}

/**
 * Does the rect reach the district's disc?
 *
 * The ground is the bounding square of the district's territory and is clipped
 * to the disc when drawn, so corner cells falling entirely outside it would be
 * invisible members of the scene graph. Dropping them here keeps hit-testing
 * and the cell budget honest about what is actually on the map.
 */
export function intersectsDisc(
  rect: TerrainRect,
  disc: { origin: GardenPosition; radius: number },
): boolean {
  const closestX = Math.max(rect.x, Math.min(disc.origin.x, rect.x + rect.width));
  const closestY = Math.max(rect.y, Math.min(disc.origin.y, rect.y + rect.height));
  const dx = closestX - disc.origin.x;
  const dy = closestY - disc.origin.y;
  return dx * dx + dy * dy <= disc.radius * disc.radius;
}

/**
 * Trailing segment of a normalized path.
 *
 * A drive or UNC root has no trailing segment and keeps its whole path: `d:`
 * labelled as the empty string would be a cell nobody could identify.
 */
export function basename(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  if (!trimmed) return path;
  const separator = trimmed.lastIndexOf("/");
  if (separator < 0) return trimmed;
  const name = trimmed.slice(separator + 1);
  return name || trimmed;
}

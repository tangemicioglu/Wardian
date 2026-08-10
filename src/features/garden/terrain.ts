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
 * ## Why cells are equal-weight
 *
 * A treemap normally weights cells by a quantity, and none is available:
 * `FileNode` carries `name`, `path`, `is_dir`, and `extension`, and a
 * directory's recursive size cannot be had without the crawl this design exists
 * to avoid. Weighting files by bytes while directories fell back to a constant
 * would make `package-lock.json` the largest thing in a repository and `src/` a
 * small square — not a defensible claim about a codebase. Equal weight says
 * "one child of this folder", which the data supports.
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
  origin: GardenPosition;
  /** How far the district reaches from its origin, from `layoutGarden`. */
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
  /** Hard ceiling on drawn cells. */
  maxCells: number;
}

/**
 * Ground radius floor.
 *
 * A district holding one agent measures a near-zero extent, and territory that
 * collapses to a point is not territory. This is the smallest plot worth
 * drawing, and it sits well inside `DISTRICT_MARGIN` so it can never reach a
 * neighbour.
 */
export const MIN_GROUND_RADIUS = 120;

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
 * Level by level, so that when `maxCells` binds the deepest level is dropped
 * *whole*. Dropping a level partially would show half of one folder's children
 * and none of its neighbour's, which reads as a claim about the two folders
 * rather than as a budget.
 */
export function buildTerrain(input: TerrainInput): TerrainCell[] {
  const cells: TerrainCell[] = [];

  interface Pending {
    cell: TerrainCell;
    /** Rect the children are laid out into, already inset. */
    inner: TerrainRect;
    /** Disc the cell's district occupies, for the visibility test. */
    disc: { origin: GardenPosition; radius: number };
  }

  let frontier: Pending[] = [];

  // Districts in id order so the output is identical run to run.
  for (const districtId of [...input.districts.keys()].sort()) {
    const district = input.districts.get(districtId);
    if (!district || district.roots.length === 0) continue;

    const radius = Math.max(district.radius, MIN_GROUND_RADIUS);
    const disc = { origin: district.origin, radius };
    const ground: TerrainRect = {
      x: district.origin.x - radius,
      y: district.origin.y - radius,
      width: radius * 2,
      height: radius * 2,
    };

    const roots = [...new Set(district.roots)].sort();
    const placed = squarify(
      roots.map((path) => ({ value: 1, datum: path })),
      ground,
    );

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
      frontier.push({ cell, inner: innerRect(rect), disc });
    }
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

      const placed = squarify(
        listing.children.map((child) => ({ value: 1, datum: child })),
        parent.inner,
      );
      let contributed = false;
      for (const { datum: child, rect } of placed) {
        if (!intersectsDisc(rect, parent.disc)) continue;
        const cell: TerrainCell = {
          path: child.path,
          name: child.name,
          isDir: child.isDir,
          districtId: parent.cell.districtId,
          depth,
          rect,
          truncated: child.isDir,
        };
        level.push(cell);
        contributed = true;
        if (child.isDir) next.push({ cell, inner: innerRect(rect), disc: parent.disc });
      }
      if (contributed) expanded.push(parent.cell);
    }

    if (level.length === 0) break;
    if (cells.length + level.length > input.maxCells) break;

    for (const parent of expanded) parent.truncated = false;
    cells.push(...level);
    frontier = next;
    depth += 1;
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

import type { GardenPosition } from "./garden.types";

/** Floor for *user* zoom, applied to the wheel. */
export const MIN_SCALE = 0.4;

/**
 * Floor for the automatic fit.
 *
 * Separate from the wheel floor because the two answer different questions. A
 * user zooming out past 0.4 by hand has gone further than is useful; a map that
 * is genuinely 11,000 units across still has to be *shown*. Clamping the fit at
 * the wheel floor rendered such a map five times too large for its viewport and
 * centred on the empty gap between districts, which looks exactly like an empty
 * Garden.
 */
export const MIN_FIT_SCALE = 0.04;

export const MAX_SCALE = 2.5;

/** Margin around the content bounds when fitting, in world units. */
export const FIT_PADDING = 80;

export interface ViewportSize {
  width: number;
  height: number;
}

export interface FitTransform {
  scale: number;
  position: GardenPosition;
}

/**
 * Scale and offset that bring every unit into view.
 *
 * The layout places units around each district's own origin, so world
 * coordinates are freely negative and a district can sit thousands of units
 * from the Stage's top-left. Opening onto a viewport that happens to contain
 * two of thirty-seven districts is not a map.
 *
 * Only ever zooms *out*: magnifying a sparse map would start the user inside
 * one cluster with no sense of the whole.
 */
export function fitTransform(
  positions: readonly GardenPosition[],
  size: ViewportSize,
): FitTransform | null {
  if (positions.length === 0 || size.width <= 0 || size.height <= 0) return null;

  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  for (const position of positions) {
    if (!Number.isFinite(position.x) || !Number.isFinite(position.y)) continue;
    minX = Math.min(minX, position.x);
    maxX = Math.max(maxX, position.x);
    minY = Math.min(minY, position.y);
    maxY = Math.max(maxY, position.y);
  }
  if (!Number.isFinite(minX) || !Number.isFinite(minY)) return null;

  minX -= FIT_PADDING;
  maxX += FIT_PADDING;
  minY -= FIT_PADDING;
  maxY += FIT_PADDING;

  const scale = Math.min(
    1,
    Math.max(MIN_FIT_SCALE, Math.min(size.width / (maxX - minX), size.height / (maxY - minY))),
  );
  return {
    scale,
    position: {
      x: size.width / 2 - ((minX + maxX) / 2) * scale,
      y: size.height / 2 - ((minY + maxY) / 2) * scale,
    },
  };
}

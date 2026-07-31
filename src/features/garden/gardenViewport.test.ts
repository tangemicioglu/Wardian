import { describe, expect, it } from "vitest";
import {
  FIT_PADDING,
  MIN_FIT_SCALE,
  MIN_SCALE,
  fitTransform,
  zoomAt,
  type FitTransform,
  type ViewportSize,
} from "./gardenViewport";

const viewport: ViewportSize = { width: 1392, height: 1044 };

/** Where a world point lands on screen under a transform. */
function project(
  transform: { scale: number; position: { x: number; y: number } },
  point: { x: number; y: number },
) {
  return {
    x: transform.position.x + point.x * transform.scale,
    y: transform.position.y + point.y * transform.scale,
  };
}

describe("fitTransform", () => {
  it("brings a map far larger than the wheel floor allows into view", () => {
    // The bug this exists to catch: the fit was clamped at the *user* zoom
    // floor, so a genuinely large map rendered several times too big for its
    // viewport and centred on empty space between districts. It looked like an
    // empty Garden.
    const corners = [
      { x: -350, y: -360 },
      { x: 4872, y: 5760 },
    ];
    const transform = fitTransform(corners, viewport)!;
    expect(transform.scale).toBeLessThan(MIN_SCALE);

    for (const corner of corners) {
      const screen = project(transform, corner);
      expect(screen.x).toBeGreaterThanOrEqual(0);
      expect(screen.x).toBeLessThanOrEqual(viewport.width);
      expect(screen.y).toBeGreaterThanOrEqual(0);
      expect(screen.y).toBeLessThanOrEqual(viewport.height);
    }
  });

  it("centres the content", () => {
    const transform = fitTransform(
      [
        { x: 100, y: 200 },
        { x: 300, y: 600 },
      ],
      viewport,
    )!;
    const middle = project(transform, { x: 200, y: 400 });
    expect(middle.x).toBeCloseTo(viewport.width / 2);
    expect(middle.y).toBeCloseTo(viewport.height / 2);
  });

  it("never magnifies a sparse map", () => {
    // Starting inside one cluster gives no sense of the whole.
    const transform = fitTransform([{ x: 0, y: 0 }], viewport)!;
    expect(transform.scale).toBe(1);
  });

  it("leaves a margin around the outermost units", () => {
    const transform = fitTransform(
      [
        { x: 0, y: 0 },
        { x: 4000, y: 3000 },
      ],
      viewport,
    )!;
    const edge = project(transform, { x: 0, y: 0 });
    expect(edge.x).toBeGreaterThan(FIT_PADDING * transform.scale * 0.9);
  });

  it("stops zooming out at a floor rather than vanishing", () => {
    const transform = fitTransform(
      [
        { x: 0, y: 0 },
        { x: 10_000_000, y: 10_000_000 },
      ],
      viewport,
    )!;
    expect(transform.scale).toBe(MIN_FIT_SCALE);
  });

  it("reports nothing to fit rather than producing a broken transform", () => {
    expect(fitTransform([], viewport)).toBeNull();
    expect(fitTransform([{ x: 0, y: 0 }], { width: 0, height: 0 })).toBeNull();
    expect(fitTransform([{ x: NaN, y: 0 }], viewport)).toBeNull();
  });

  it("ignores a non-finite outlier instead of collapsing the whole view", () => {
    const transform = fitTransform(
      [
        { x: 0, y: 0 },
        { x: 200, y: 200 },
        { x: Infinity, y: 0 },
      ],
      viewport,
    )!;
    expect(Number.isFinite(transform.scale)).toBe(true);
    expect(Number.isFinite(transform.position.x)).toBe(true);
  });
});

describe("zoomAt", () => {
  const bounds = { min: 0.04, max: 2.5 };
  const start = { scale: 1, position: { x: 120, y: -40 } };

  /** Where a world point lands on screen under a transform. */
  const project = (t: FitTransform, p: { x: number; y: number }) => ({
    x: t.position.x + p.x * t.scale,
    y: t.position.y + p.y * t.scale,
  });
  /** The world point currently under a screen point. */
  const unproject = (t: FitTransform, p: { x: number; y: number }) => ({
    x: (p.x - t.position.x) / t.scale,
    y: (p.y - t.position.y) / t.scale,
  });

  it("keeps the point under the cursor exactly where it was", () => {
    // The bug this exists to catch: scaling without moving the stage leaves only
    // the stage origin fixed, so the map slides across the viewport as it grows.
    // Zooming then reads as scrolling, which is precisely how it was reported.
    const cursor = { x: 700, y: 300 };
    const before = unproject(start, cursor);
    for (const factor of [1.05, 1 / 1.05, 1.25, 1 / 1.25, 4, 0.25]) {
      const after = zoomAt(cursor, start, factor, bounds);
      const screen = project(after, before);
      expect(screen.x).toBeCloseTo(cursor.x);
      expect(screen.y).toBeCloseTo(cursor.y);
    }
  });

  it("holds the anchor across a whole gesture, not just one notch", () => {
    // Rounding that is invisible in one step is a visible drift over the twenty
    // or so events a single scroll produces.
    const cursor = { x: 401, y: 277 };
    const anchor = unproject(start, cursor);
    let transform = start;
    for (let i = 0; i < 40; i += 1) {
      transform = zoomAt(cursor, transform, i % 2 === 0 ? 1.05 : 1.03, bounds);
    }
    const screen = project(transform, anchor);
    expect(screen.x).toBeCloseTo(cursor.x, 3);
    expect(screen.y).toBeCloseTo(cursor.y, 3);
  });

  it("scales by the factor it is given", () => {
    expect(zoomAt({ x: 0, y: 0 }, start, 1.25, bounds).scale).toBeCloseTo(1.25);
    expect(zoomAt({ x: 0, y: 0 }, start, 0.5, bounds).scale).toBeCloseTo(0.5);
  });

  it("clamps to the bounds rather than running away", () => {
    expect(zoomAt({ x: 10, y: 10 }, start, 1000, bounds).scale).toBe(bounds.max);
    expect(zoomAt({ x: 10, y: 10 }, start, 0.00001, bounds).scale).toBe(bounds.min);
  });

  it("still anchors correctly when the zoom is clamped", () => {
    // Clamping changes the factor actually applied; the position has to follow
    // the applied scale, not the requested one, or the map jumps at the limit.
    const cursor = { x: 250, y: 250 };
    const anchor = unproject(start, cursor);
    const clamped = zoomAt(cursor, start, 1000, bounds);
    const screen = project(clamped, anchor);
    expect(screen.x).toBeCloseTo(cursor.x);
    expect(screen.y).toBeCloseTo(cursor.y);
  });

  it("returns the transform untouched when it is already at the limit", () => {
    const atMax = { scale: bounds.max, position: { x: 5, y: 6 } };
    expect(zoomAt({ x: 0, y: 0 }, atMax, 2, bounds)).toEqual(atMax);
  });

  it("refuses to produce a broken transform from a degenerate one", () => {
    const broken = { scale: 0, position: { x: 0, y: 0 } };
    expect(zoomAt({ x: 1, y: 1 }, broken, 2, bounds)).toEqual(broken);
  });
});

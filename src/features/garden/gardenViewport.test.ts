import { describe, expect, it } from "vitest";
import {
  FIT_PADDING,
  MIN_FIT_SCALE,
  MIN_SCALE,
  fitTransform,
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

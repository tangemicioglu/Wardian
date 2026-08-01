import { describe, expect, it, vi } from "vitest";
import { renderHook } from "@testing-library/react";
import type Konva from "konva";
import { Animation } from "konva/lib/Animation";
import { PULSE_HALO_NAME, pulseScale, useGardenPulse } from "./useGardenPulse";

/**
 * Minimal stand-ins for the Konva nodes the hook touches.
 *
 * Real `Konva.Layer`/`Konva.Circle` instances would drag in a canvas context
 * jsdom does not implement; the hook only ever calls `find`, `scale`,
 * `getStage`, and `batchDraw`, so those are what the fakes provide.
 */
function fakeShape(name = PULSE_HALO_NAME) {
  let current = { x: 1, y: 1 };
  return {
    name,
    scale: vi.fn((next?: { x: number; y: number }) => {
      if (next) current = next;
      return current;
    }),
    getStage: () => ({}),
    get scaleX() {
      return current.x;
    },
  };
}

function fakeLayer(shapes: ReturnType<typeof fakeShape>[]) {
  return {
    find: (selector: string) =>
      shapes.filter((shape) => selector === `.${shape.name}`),
    batchDraw: vi.fn(),
  } as unknown as Konva.Layer;
}

describe("pulseScale", () => {
  it("breathes around 1 without ever inverting or vanishing", () => {
    for (const seconds of [0, 0.25, 0.5, 1, 1.5, 2, 7.3]) {
      expect(pulseScale(seconds)).toBeGreaterThan(0.9);
      expect(pulseScale(seconds)).toBeLessThan(1.1);
    }
    expect(pulseScale(0)).toBe(1);
    expect(pulseScale(0.5)).toBeGreaterThan(1);
  });
});

describe("useGardenPulse", () => {
  it("starts no animation when nothing is active, protecting idle CPU", () => {
    const start = vi.spyOn(Animation.prototype, "start");
    const layer = fakeLayer([fakeShape("something-else")]);
    renderHook(() => useGardenPulse({ current: layer }, 0));
    expect(start).not.toHaveBeenCalled();
    start.mockRestore();
  });

  it("drives every tagged shape from one animation rather than one per unit", () => {
    // The whole point: N busy units must cost one animation, not N.
    const start = vi.spyOn(Animation.prototype, "start").mockImplementation(function (
      this: Animation,
    ) {
      return this;
    });
    const layer = fakeLayer([fakeShape(), fakeShape(), fakeShape(), fakeShape(), fakeShape()]);
    renderHook(() => useGardenPulse({ current: layer }, 0));
    expect(start).toHaveBeenCalledTimes(1);
    start.mockRestore();
  });

  it("scales every tagged shape on a frame, without re-rendering", () => {
    const start = vi.spyOn(Animation.prototype, "start").mockImplementation(function (
      this: Animation,
    ) {
      return this;
    });
    const shapes = [fakeShape(), fakeShape(), fakeShape()];
    let renders = 0;
    renderHook(() => {
      renders += 1;
      useGardenPulse({ current: fakeLayer(shapes) }, 0);
    });
    const rendersAfterMount = renders;

    // Drive one frame the way Konva's ticker would.
    const animation = start.mock.instances[0] as Animation;
    (animation.func as (frame: { time: number }) => void)({ time: 500 });

    for (const shape of shapes) {
      expect(shape.scaleX).toBeCloseTo(pulseScale(0.5));
    }
    expect(renders).toBe(rendersAfterMount);
    start.mockRestore();
  });

  it("leaves the shapes at rest when it stops", () => {
    // A stopped animation would otherwise freeze them mid-breath.
    const start = vi.spyOn(Animation.prototype, "start").mockImplementation(function (
      this: Animation,
    ) {
      return this;
    });
    const stop = vi.spyOn(Animation.prototype, "stop").mockImplementation(function (
      this: Animation,
    ) {
      return this;
    });
    const shapes = [fakeShape(), fakeShape()];
    const { unmount } = renderHook(() => useGardenPulse({ current: fakeLayer(shapes) }, 0));
    for (const shape of shapes) shape.scale({ x: 1.08, y: 1.08 });

    unmount();

    expect(stop).toHaveBeenCalled();
    expect(shapes[0].scaleX).toBe(1);
    expect(shapes[1].scaleX).toBe(1);
    start.mockRestore();
    stop.mockRestore();
  });
});

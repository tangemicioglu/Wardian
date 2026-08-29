import { useEffect, type RefObject } from "react";
// Submodule import on purpose. The `konva` package entry resolves to its Node
// build under a test runner, which requires the native `canvas` package and
// fails to load; the animation class itself needs neither.
import { Animation } from "konva/lib/Animation";
import type Konva from "konva";

/** Konva node `name` on the shapes the shared pulse animates. */
export const PULSE_HALO_NAME = "pulse-halo";

/** Resting radius of an agent's status halo. */
export const PULSE_BASE_RADIUS = 18;

/** Amplitude of the breathing, as a fraction of the resting size. */
const PULSE_AMPLITUDE = 0.08;

/** Breathing multiplier at time `seconds`. Exported so the shape is testable. */
export function pulseScale(seconds: number): number {
  return 1 + PULSE_AMPLITUDE * Math.sin(seconds * Math.PI);
}

/**
 * Breathe every active unit's halo from a single animation.
 *
 * Previously each unit ran its own `requestAnimationFrame` loop driving a React
 * state update, so N active units meant N component re-renders per frame. That
 * was survivable when a unit was three Konva nodes; once agents grew a skill
 * crown it became the Garden's dominant frame cost, because every frame
 * reconciled the whole crown of every busy agent in order to move one circle by
 * a pixel.
 *
 * Canvas animation belongs on the canvas. This scales the tagged shapes
 * directly and lets Konva redraw the layer once, so the pulse costs nothing in
 * React and does not scale with what else a unit draws. The node list is cached
 * and refreshed only when the roster changes, so no frame walks the scene graph.
 *
 * Scale rather than a per-shape dimension, because the tagged shapes are not all
 * circles: an agent's halo is a circle centred on the unit origin and a
 * automation's is a rounded rectangle, and both read correctly under a uniform
 * scale about that origin.
 *
 * The animation is not started at all when nothing is active, which preserves
 * the original idle-CPU guarantee.
 */
export function useGardenPulse(
  layerRef: RefObject<Konva.Layer | null>,
  /** Changes whenever the units — or their statuses — may have changed. */
  revision: unknown,
): void {
  useEffect(() => {
    const layer = layerRef.current;
    if (!layer) return;

    const shapes = layer.find<Konva.Shape>(`.${PULSE_HALO_NAME}`);
    if (shapes.length === 0) return;

    const animation = new Animation((frame) => {
      const scale = pulseScale((frame?.time ?? 0) / 1000);
      for (const shape of shapes) shape.scale({ x: scale, y: scale });
    }, layer);
    animation.start();

    return () => {
      animation.stop();
      // Leave the shapes at rest; a stopped animation would otherwise freeze
      // them mid-breath at whatever scale the last frame set.
      for (const shape of shapes) {
        if (shape.getStage()) shape.scale({ x: 1, y: 1 });
      }
      layer.batchDraw();
    };
  }, [layerRef, revision]);
}

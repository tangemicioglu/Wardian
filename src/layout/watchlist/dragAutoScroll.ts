import { useEffect, useRef, type RefObject } from "react";

/** Distance, in pixels, from a container edge where auto-scrolling engages. */
export const DEFAULT_AUTO_SCROLL_EDGE = 56;

/** Peak auto-scroll velocity in pixels per second, reached at the outer edge. */
export const DEFAULT_AUTO_SCROLL_SPEED = 900;

export interface AutoScrollBounds {
  top: number;
  height: number;
}

export interface AutoScrollOptions {
  /** Height of the hot zone at each edge. Clamped to half the container height. */
  edgeSize?: number;
  /** Velocity in px/s applied when the pointer sits at (or beyond) the edge. */
  maxSpeed?: number;
}

/**
 * Velocity the list should scroll at while a drag hovers near a container edge.
 *
 * Returns pixels per second: negative scrolls toward the top of the list,
 * positive toward the bottom, zero when the pointer sits in the neutral middle
 * band. The ramp is quadratic so nudging into the hot zone creeps and burying
 * the cursor past the edge runs at full speed, which keeps long lists
 * controllable instead of snapping past the intended drop row.
 */
export function computeAutoScrollSpeed(
  pointerY: number,
  bounds: AutoScrollBounds,
  options: AutoScrollOptions = {},
): number {
  const { top, height } = bounds;
  if (!Number.isFinite(pointerY) || height <= 0) return 0;

  const maxSpeed = options.maxSpeed ?? DEFAULT_AUTO_SCROLL_SPEED;
  const edgeSize = Math.min(options.edgeSize ?? DEFAULT_AUTO_SCROLL_EDGE, height / 2);
  if (edgeSize <= 0 || maxSpeed <= 0) return 0;

  const ramp = (distance: number) => {
    const intensity = Math.min(Math.max((edgeSize - distance) / edgeSize, 0), 1);
    return intensity * intensity * maxSpeed;
  };

  const distanceFromTop = pointerY - top;
  if (distanceFromTop < edgeSize) return -ramp(distanceFromTop);

  const distanceFromBottom = top + height - pointerY;
  if (distanceFromBottom < edgeSize) return ramp(distanceFromBottom);

  return 0;
}

/**
 * Scrolls `containerRef` while `active` is true and the pointer rests near one
 * of its vertical edges, so a drag can reach rows outside the current viewport.
 *
 * The loop runs on `requestAnimationFrame` and mutates `scrollTop` directly:
 * no React state changes per frame, so the surrounding list is not re-rendered
 * while scrolling. Frame deltas are clamped so a stalled tab cannot resume with
 * one huge jump.
 */
export function useDragAutoScroll(
  containerRef: RefObject<HTMLElement | null>,
  active: boolean,
  options: AutoScrollOptions = {},
): void {
  const edgeSize = options.edgeSize;
  const maxSpeed = options.maxSpeed;
  const pointerYRef = useRef<number | null>(null);

  useEffect(() => {
    if (!active) {
      pointerYRef.current = null;
      return;
    }
    if (typeof window === "undefined" || typeof window.requestAnimationFrame !== "function") return;

    const handlePointerMove = (event: MouseEvent) => {
      pointerYRef.current = event.clientY;
    };
    // Capture phase: watchlist rows call stopPropagation on their own mousemove
    // handlers, which halts the native event at React's root container before a
    // bubbling window listener would ever see it.
    window.addEventListener("mousemove", handlePointerMove, true);

    let frame = 0;
    let lastTimestamp = 0;
    const step = (timestamp: number) => {
      const container = containerRef.current;
      const pointerY = pointerYRef.current;
      const elapsed = lastTimestamp ? Math.min((timestamp - lastTimestamp) / 1000, 0.05) : 0;
      lastTimestamp = timestamp;

      if (container && pointerY !== null && elapsed > 0) {
        const rect = container.getBoundingClientRect();
        const speed = computeAutoScrollSpeed(pointerY, { top: rect.top, height: rect.height }, { edgeSize, maxSpeed });
        if (speed !== 0) container.scrollTop += speed * elapsed;
      }

      frame = window.requestAnimationFrame(step);
    };
    frame = window.requestAnimationFrame(step);

    return () => {
      window.removeEventListener("mousemove", handlePointerMove, true);
      window.cancelAnimationFrame(frame);
      pointerYRef.current = null;
    };
  }, [active, containerRef, edgeSize, maxSpeed]);
}

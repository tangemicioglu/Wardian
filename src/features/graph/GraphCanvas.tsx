import { Suspense, lazy } from "react";

import type { GraphCanvasProps } from "./GraphCanvasImpl";

export type { GraphCanvasProps } from "./GraphCanvasImpl";

/**
 * Sigma and graphology, kept out of the startup bundle.
 *
 * Together they are 231 KB minified that every launch parsed whether or not the
 * Graph was ever opened. This module is the boundary rather than `GraphView`,
 * because the view's own logic is small and its callers mock this path.
 *
 * The chunk is fetched when `rendererActive` first turns true, which
 * `SuspendedSurfaceRenderer` does from an effect rather than during the reveal
 * render — so the request starts after the surface has painted, not before.
 */
const GraphCanvasImpl = lazy(async () => ({
  default: (await import("./GraphCanvasImpl")).GraphCanvas,
}));

export function GraphCanvas(props: GraphCanvasProps) {
  return (
    <Suspense
      fallback={<div className="graph-empty-state" data-testid="graph-canvas-loading" />}
    >
      <GraphCanvasImpl {...props} />
    </Suspense>
  );
}

import { Suspense, lazy } from "react";

import type { GardenCanvasProps } from "./GardenCanvasImpl";

export type { GardenCanvasProps } from "./GardenCanvasImpl";

/**
 * Konva and react-konva, kept out of the startup bundle.
 *
 * The Garden is the only surface that draws with them, and it is not the
 * surface most launches open. `GardenView` already renders a placeholder at the
 * canvas's final geometry while `rendererActive` is false, so the fallback here
 * only has to hold that same space for the length of the fetch.
 */
const GardenCanvasImpl = lazy(async () => ({
  default: (await import("./GardenCanvasImpl")).GardenCanvas,
}));

export function GardenCanvas(props: GardenCanvasProps) {
  return (
    <Suspense
      fallback={(
        <div
          className="flex flex-1 items-center justify-center text-sm text-muted"
          data-testid="garden-canvas-loading"
        />
      )}
    >
      <GardenCanvasImpl {...props} />
    </Suspense>
  );
}

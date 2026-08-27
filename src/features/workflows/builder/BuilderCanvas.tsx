import { Suspense, lazy } from 'react';

import type { BuilderCanvasProps } from './BuilderCanvasImpl';

export type { BuilderCanvasProps } from './BuilderCanvasImpl';

/**
 * `@xyflow/react` and its stylesheet, kept out of the startup bundle.
 *
 * The workflow builder and the run DAG are the only things that draw with it,
 * and neither is on the path a launch takes. Both boundaries point at this same
 * dependency, so both have to be dynamic for the chunk to leave startup.
 */
const BuilderCanvasImpl = lazy(async () => ({
  default: (await import('./BuilderCanvasImpl')).BuilderCanvas,
}));

export function BuilderCanvas(props: BuilderCanvasProps) {
  return (
    <Suspense fallback={<div className="flex-1" data-testid="builder-canvas-loading" />}>
      <BuilderCanvasImpl {...props} />
    </Suspense>
  );
}

import { Suspense, lazy } from 'react';

import type { RunDagProps } from './RunDagImpl';

export type { RunDagProps } from './RunDagImpl';

/** The run DAG's half of the `@xyflow/react` split; see `BuilderCanvas`. */
const RunDagImpl = lazy(async () => ({
  default: (await import('./RunDagImpl')).RunDag,
}));

export function RunDag(props: RunDagProps) {
  return (
    <Suspense fallback={<div className="flex-1" data-testid="run-dag-loading" />}>
      <RunDagImpl {...props} />
    </Suspense>
  );
}

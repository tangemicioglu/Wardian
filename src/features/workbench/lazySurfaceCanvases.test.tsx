import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { GraphCanvas, type GraphCanvasProps } from "../graph/GraphCanvas";
import { GardenCanvas, type GardenCanvasProps } from "../garden/GardenCanvas";
import { BuilderCanvas, type BuilderCanvasProps } from "../automations/builder/BuilderCanvas";
import { RunDag, type RunDagProps } from "../automations/run/RunDag";

/**
 * These wrappers forward props untouched and never read them, so a placeholder
 * is enough to render one. Building valid props would mean importing the very
 * modules these boundaries exist to keep out of the first render.
 */
const forwarded = {} as never;

describe("lazy surface canvases", () => {
  it.each([
    ["graph", "graph-canvas-loading", () => (
      <GraphCanvas {...(forwarded as GraphCanvasProps)} />
    )],
    ["garden", "garden-canvas-loading", () => (
      <GardenCanvas {...(forwarded as GardenCanvasProps)} />
    )],
    ["automation builder", "builder-canvas-loading", () => (
      <BuilderCanvas {...(forwarded as BuilderCanvasProps)} />
    )],
    ["run DAG", "run-dag-loading", () => (
      <RunDag {...(forwarded as RunDagProps)} />
    )],
  ])("keeps the %s renderer out of the first render pass", (_label, testId, element) => {
    render(element());

    // Sigma, Konva and xyflow are ~430 KB of vendor code between them, and every
    // launch parsed all of it. Suspending here proves the module is still being
    // fetched when the surface first paints, rather than blocking that paint.
    expect(screen.getByTestId(testId)).toBeInTheDocument();
  });
});

import { describe, expect, it } from "vitest";

import { threadKey, threadsFor } from "./attributionThreads";
import type { TerrainCell } from "./terrain";
import type { TerrainPaint } from "./terrainPaint";
import type { GardenPosition } from "./garden.types";

function cell(path: string, x = 0, y = 0): TerrainCell {
  return {
    path,
    name: path.slice(path.lastIndexOf("/") + 1),
    isDir: false,
    districtId: "workspace:d:/repo",
    depth: 1,
    rect: { x, y, width: 10, height: 10 },
    truncated: false,
  };
}

function paint(overrides: Partial<TerrainPaint> = {}): TerrainPaint {
  return {
    kind: "modified",
    churn: 0.5,
    recency: 1,
    evidence: "attributed",
    reviewed: false,
    agentIds: ["a1"],
    count: 1,
    ...overrides,
  };
}

const positions = new Map<string, GardenPosition>([
  ["a1", { x: 100, y: 100 }],
  ["a2", { x: 200, y: 200 }],
]);

describe("threadsFor", () => {
  it("draws nothing without a selection", () => {
    expect(
      threadsFor({
        cells: [cell("d:/repo/a.ts")],
        paint: new Map([["d:/repo/a.ts", paint()]]),
        positions,
      }),
    ).toEqual([]);
  });

  it("draws an agent's writes when the agent is selected", () => {
    const threads = threadsFor({
      cells: [cell("d:/repo/a.ts"), cell("d:/repo/b.ts", 40)],
      paint: new Map([
        ["d:/repo/a.ts", paint({ agentIds: ["a1"] })],
        ["d:/repo/b.ts", paint({ agentIds: ["a2"] })],
      ]),
      positions,
      selectedAgentId: "a1",
    });
    expect(threads).toHaveLength(1);
    expect(threads[0]).toMatchObject({
      key: threadKey("a1", "d:/repo/a.ts"),
      from: { x: 100, y: 100 },
      to: { x: 5, y: 5 },
    });
  });

  it("draws every claimant when a cell is selected", () => {
    const threads = threadsFor({
      cells: [cell("d:/repo/a.ts")],
      paint: new Map([["d:/repo/a.ts", paint({ agentIds: ["a1", "a2"] })]]),
      positions,
      selectedPath: "d:/repo/a.ts",
    });
    expect(threads.map((thread) => thread.key).sort()).toEqual([
      threadKey("a1", "d:/repo/a.ts"),
      threadKey("a2", "d:/repo/a.ts"),
    ]);
  });

  it("never threads an inferred write to an agent", () => {
    // A shell-driven write is a change nobody claimed. Drawing a line would
    // invent the attribution the evidence discriminant exists to withhold.
    const threads = threadsFor({
      cells: [cell("d:/repo/a.ts")],
      paint: new Map([
        ["d:/repo/a.ts", paint({ evidence: "inferred", agentIds: [] })],
      ]),
      positions,
      selectedPath: "d:/repo/a.ts",
    });
    expect(threads).toEqual([]);
  });

  it("skips an agent that is not on the map", () => {
    const threads = threadsFor({
      cells: [cell("d:/repo/a.ts")],
      paint: new Map([["d:/repo/a.ts", paint({ agentIds: ["ghost"] })]]),
      positions,
      selectedPath: "d:/repo/a.ts",
    });
    expect(threads).toEqual([]);
  });

  it("caps the bundle by churn rather than by enumeration order", () => {
    const cells = Array.from({ length: 40 }, (_, index) =>
      cell(`d:/repo/${String(index).padStart(2, "0")}.ts`, index * 20),
    );
    const paints = new Map(
      cells.map((entry, index) => [entry.path, paint({ churn: index / 40 })]),
    );
    const threads = threadsFor({
      cells,
      paint: paints,
      positions,
      selectedAgentId: "a1",
      max: 5,
    });
    expect(threads).toHaveLength(5);
    // The five busiest, not the first five alphabetically.
    expect(threads[0].key).toBe(threadKey("a1", "d:/repo/39.ts"));
  });

  it("threads the file, not every folder above it", () => {
    // `buildTerrainPaint` rolls a change onto every ancestor and pools the agent
    // ids up the chain, so without supersession one write drew four lines making
    // one claim — and the folders' churn is the sum of their subtree, so they
    // outranked the file and the cap kept the least specific ones.
    const cells = [
      cell("d:/repo", 0),
      cell("d:/repo/src", 20),
      cell("d:/repo/src/features", 40),
      cell("d:/repo/src/features/row.tsx", 60),
    ];
    const paints = new Map([
      ["d:/repo", paint({ churn: 1 })],
      ["d:/repo/src", paint({ churn: 0.8 })],
      ["d:/repo/src/features", paint({ churn: 0.6 })],
      ["d:/repo/src/features/row.tsx", paint({ churn: 0.2 })],
    ]);
    const threads = threadsFor({ cells, paint: paints, positions, selectedAgentId: "a1" });
    expect(threads.map((thread) => thread.key)).toEqual([
      threadKey("a1", "d:/repo/src/features/row.tsx"),
    ]);
  });

  it("keeps a folder's thread when it is the deepest cell rendered", () => {
    // At a coarse level of detail "the write is somewhere in here" is the
    // truthful answer, and dropping the thread would lose the tie entirely.
    const cells = [cell("d:/repo", 0), cell("d:/repo/src", 20)];
    const paints = new Map([
      ["d:/repo", paint({ churn: 1 })],
      ["d:/repo/src", paint({ churn: 0.8 })],
    ]);
    const threads = threadsFor({ cells, paint: paints, positions, selectedAgentId: "a1" });
    expect(threads.map((thread) => thread.key)).toEqual([threadKey("a1", "d:/repo/src")]);
  });

  it("still threads a selected folder even though its children are rendered", () => {
    // Selecting a piece of ground asks "who wrote *this*", so supersession must
    // not answer a different question.
    const cells = [cell("d:/repo/src", 0), cell("d:/repo/src/row.tsx", 20)];
    const paints = new Map([
      ["d:/repo/src", paint({ churn: 1, agentIds: ["a1"] })],
      ["d:/repo/src/row.tsx", paint({ churn: 0.2, agentIds: ["a1"] })],
    ]);
    const threads = threadsFor({
      cells,
      paint: paints,
      positions,
      selectedPath: "d:/repo/src",
    });
    expect(threads.map((thread) => thread.key)).toEqual([threadKey("a1", "d:/repo/src")]);
  });

  it("is deterministic when churn ties", () => {
    const cells = [cell("d:/repo/b.ts"), cell("d:/repo/a.ts", 40)];
    const paints = new Map(cells.map((entry) => [entry.path, paint({ churn: 0.5 })]));
    const threads = threadsFor({ cells, paint: paints, positions, selectedAgentId: "a1", max: 1 });
    expect(threads[0].key).toBe(threadKey("a1", "d:/repo/a.ts"));
  });
});

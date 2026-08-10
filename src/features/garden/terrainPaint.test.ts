import { describe, expect, it } from "vitest";

import type { ChangeReviewFileEntry } from "../../types";
import {
  MAX_CHANGE_ALPHA,
  MIN_CHANGE_ALPHA,
  RECENCY_HALF_LIFE_TURNS,
  ancestorChain,
  buildTerrainPaint,
  changeAlpha,
  joinWorkspacePath,
  type RootChangeSet,
  type TerrainPaint,
} from "./terrainPaint";

const ROOT = "d:/work/repo";

function entry(overrides: Partial<ChangeReviewFileEntry> = {}): ChangeReviewFileEntry {
  return {
    path: "src/a.ts",
    change_kind: "modified",
    old_path: null,
    insertions: 5,
    deletions: 2,
    evidence: "attributed",
    agent_ids: ["a1"],
    turn_indices: [10],
    binary: false,
    truncated: false,
    reviewed: false,
    ...overrides,
  };
}

function changeSet(entries: ChangeReviewFileEntry[], toTurnIndex: number | null = 10): RootChangeSet {
  return { root: ROOT, entries, toTurnIndex };
}

describe("changeAlpha", () => {
  function paint(overrides: Partial<TerrainPaint> = {}): TerrainPaint {
    return {
      kind: "modified",
      churn: 1,
      recency: 1,
      evidence: "attributed",
      reviewed: false,
      agentIds: ["a1"],
      count: 1,
      ...overrides,
    };
  }

  it("stays within the band at both extremes", () => {
    expect(changeAlpha(paint(), false)).toBeCloseTo(MAX_CHANGE_ALPHA, 6);
    expect(changeAlpha(paint({ churn: 0, recency: 0 }), false)).toBeCloseTo(MIN_CHANGE_ALPHA, 6);
  });

  it("rises with churn and with recency", () => {
    const quiet = changeAlpha(paint({ churn: 0.1, recency: 0.1 }), false);
    expect(changeAlpha(paint({ churn: 0.9, recency: 0.1 }), false)).toBeGreaterThan(quiet);
    expect(changeAlpha(paint({ churn: 0.1, recency: 0.9 }), false)).toBeGreaterThan(quiet);
  });

  it("steps a folder back once its children carry the same signal", () => {
    // Tints composite, so a folder painting at full strength on top of its
    // children would put nesting depth into the channel that means churn.
    expect(changeAlpha(paint(), true)).toBeLessThan(changeAlpha(paint(), false));
  });

  it("dims reviewed work rather than hiding it", () => {
    const reviewed = changeAlpha(paint({ reviewed: true }), false);
    expect(reviewed).toBeGreaterThan(0);
    expect(reviewed).toBeLessThan(changeAlpha(paint(), false));
  });
});

describe("ancestorChain", () => {
  it("returns the path and every ancestor up to the root", () => {
    expect(ancestorChain("d:/work/repo/src/deep/a.ts", ROOT)).toEqual([
      "d:/work/repo/src/deep/a.ts",
      "d:/work/repo/src/deep",
      "d:/work/repo/src",
      "d:/work/repo",
    ]);
  });

  it("returns just the root for the root itself", () => {
    expect(ancestorChain(ROOT, ROOT)).toEqual([ROOT]);
  });

  it("refuses to climb out of a path that is not under its root", () => {
    // Otherwise one root's entry would paint every ancestor up to the drive.
    expect(ancestorChain("d:/other/a.ts", ROOT)).toEqual(["d:/other/a.ts"]);
  });

  it("does not treat a sibling with a shared prefix as a descendant", () => {
    expect(ancestorChain("d:/work/repo-two/a.ts", ROOT)).toEqual(["d:/work/repo-two/a.ts"]);
  });
});

describe("joinWorkspacePath", () => {
  it("joins a workspace-relative path onto its root", () => {
    expect(joinWorkspacePath(ROOT, "src/a.ts")).toBe("d:/work/repo/src/a.ts");
    expect(joinWorkspacePath(ROOT, "src\\a.ts")).toBe("d:/work/repo/src/a.ts");
  });

  it("leaves an already-absolute path alone", () => {
    expect(joinWorkspacePath(ROOT, "D:\\elsewhere\\a.ts")).toBe("d:/elsewhere/a.ts");
  });

  it("rejects an empty path rather than returning the root", () => {
    expect(joinWorkspacePath(ROOT, "   ")).toBeNull();
  });
});

describe("buildTerrainPaint", () => {
  it("paints the changed path and rolls it up to the root", () => {
    const paint = buildTerrainPaint([changeSet([entry()])]);
    expect([...paint.keys()].sort()).toEqual([
      "d:/work/repo",
      "d:/work/repo/src",
      "d:/work/repo/src/a.ts",
    ]);
    expect(paint.get("d:/work/repo/src")?.count).toBe(1);
  });

  it("reports a folder holding one kind as that kind", () => {
    const paint = buildTerrainPaint([
      changeSet([entry({ path: "src/a.ts" }), entry({ path: "src/b.ts" })]),
    ]);
    expect(paint.get("d:/work/repo/src")?.kind).toBe("modified");
  });

  it("reports a folder holding several kinds as mixed", () => {
    const paint = buildTerrainPaint([
      changeSet([entry({ path: "src/a.ts" }), entry({ path: "src/b.ts", change_kind: "added" })]),
    ]);
    expect(paint.get("d:/work/repo/src")?.kind).toBe("mixed");
    expect(paint.get("d:/work/repo/src/a.ts")?.kind).toBe("modified");
  });

  it("carries a deleted path on its parent without inventing a cell for it", () => {
    const paint = buildTerrainPaint([
      changeSet([entry({ path: "src/gone.ts", change_kind: "deleted" })]),
    ]);
    // The entry for the deleted path exists but no terrain cell can ever match
    // it, because the file is not on disk. The parent is where it is seen.
    expect(paint.get("d:/work/repo/src")?.kind).toBe("deleted");
    expect(paint.get("d:/work/repo/src")?.count).toBe(1);
  });

  it("sums churn up the tree and normalizes across every root", () => {
    const paint = buildTerrainPaint([
      changeSet([entry({ path: "src/a.ts", insertions: 1, deletions: 0 })]),
      {
        root: "d:/work/other",
        entries: [entry({ path: "big.ts", insertions: 1000, deletions: 0 })],
        toTurnIndex: 10,
      },
    ]);
    // A quiet repository must not read as hot as a busy one just because it is
    // the busiest thing in its own root.
    expect(paint.get("d:/work/other")?.churn).toBeCloseTo(1, 6);
    expect(paint.get("d:/work/repo")?.churn).toBeLessThan(0.15);
  });

  it("decays recency by turn distance", () => {
    const fresh = buildTerrainPaint([changeSet([entry({ turn_indices: [10] })], 10)]);
    const stale = buildTerrainPaint([
      changeSet([entry({ turn_indices: [10 - RECENCY_HALF_LIFE_TURNS] })], 10),
    ]);
    expect(fresh.get("d:/work/repo/src/a.ts")?.recency).toBeCloseTo(1, 6);
    expect(stale.get("d:/work/repo/src/a.ts")?.recency).toBeCloseTo(0.5, 6);
  });

  it("gives a shell-driven write a middling recency rather than hiding it", () => {
    const paint = buildTerrainPaint([
      changeSet([entry({ evidence: "inferred", agent_ids: [], turn_indices: [] })]),
    ]);
    const cell = paint.get("d:/work/repo/src/a.ts");
    expect(cell?.evidence).toBe("inferred");
    expect(cell?.recency).toBeGreaterThan(0);
    expect(cell?.recency).toBeLessThan(1);
  });

  it("marks a folder attributed when any path beneath it is", () => {
    const paint = buildTerrainPaint([
      changeSet([
        entry({ path: "src/a.ts", evidence: "inferred", agent_ids: [] }),
        entry({ path: "src/b.ts", evidence: "attributed", agent_ids: ["a2"] }),
      ]),
    ]);
    expect(paint.get("d:/work/repo/src")?.evidence).toBe("attributed");
    expect(paint.get("d:/work/repo/src")?.agentIds).toEqual(["a2"]);
    expect(paint.get("d:/work/repo/src/a.ts")?.evidence).toBe("inferred");
  });

  it("marks a folder reviewed only when every path beneath it is", () => {
    const partly = buildTerrainPaint([
      changeSet([
        entry({ path: "src/a.ts", reviewed: true }),
        entry({ path: "src/b.ts", reviewed: false }),
      ]),
    ]);
    expect(partly.get("d:/work/repo/src")?.reviewed).toBe(false);

    const fully = buildTerrainPaint([
      changeSet([
        entry({ path: "src/a.ts", reviewed: true }),
        entry({ path: "src/b.ts", reviewed: true }),
      ]),
    ]);
    expect(fully.get("d:/work/repo/src")?.reviewed).toBe(true);
  });

  it("unions the agents that wrote beneath a folder", () => {
    const paint = buildTerrainPaint([
      changeSet([
        entry({ path: "src/a.ts", agent_ids: ["a1"] }),
        entry({ path: "src/b.ts", agent_ids: ["a2", "a1"] }),
      ]),
    ]);
    expect(paint.get("d:/work/repo/src")?.agentIds).toEqual(["a1", "a2"]);
  });

  it("handles an empty change set and a zero-churn one", () => {
    expect(buildTerrainPaint([]).size).toBe(0);
    const paint = buildTerrainPaint([
      changeSet([entry({ insertions: null, deletions: null, change_kind: "untracked" })]),
    ]);
    expect(paint.get("d:/work/repo")?.churn).toBe(0);
    expect(paint.get("d:/work/repo")?.kind).toBe("untracked");
  });
});

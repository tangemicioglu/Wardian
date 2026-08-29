import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadGardenAutomationInputs, mergeAutomationRunStatus, resetGardenAutomationCacheForTests } from "./useGardenAutomations";
import type { RunSummary } from "../automations/run/runTypes";

const emptyContext = {
  agentIds: [],
  roleNames: [],
  classNames: [],
  workspacePaths: [],
  libraryFolder: null,
};
const emptyInput = {
  agentIds: [],
  roleNames: [],
  classNames: [],
  workspacePaths: [],
  libraryFolder: null,
};

const run = (over: Partial<RunSummary>): RunSummary => ({
  run_id: "r", blueprint_id: "w1", status: "completed", node_count: 1, path: "p", ...over,
});

describe("mergeAutomationRunStatus", () => {
  it("attaches the latest run status by updated_at", () => {
    const blueprints = [{ id: "w1", name: "Build", nodeCount: 2, context: emptyContext }];
    const runs = [
      run({ status: "completed", updated_at: "2026-06-01T00:00:00Z" }),
      run({ status: "running", updated_at: "2026-06-02T00:00:00Z" }),
    ];
    expect(mergeAutomationRunStatus(blueprints, runs)).toEqual([
      { id: "w1", label: "Build", runStatus: "running", nodeCount: 2, ...emptyInput },
    ]);
  });

  it("reports 'none' for a blueprint that has never run", () => {
    const blueprints = [{ id: "w2", name: "Ship", nodeCount: 1, context: emptyContext }];
    expect(mergeAutomationRunStatus(blueprints, [])).toEqual([
      { id: "w2", label: "Ship", runStatus: "none", nodeCount: 1, ...emptyInput },
    ]);
  });
});

describe("loadGardenAutomationInputs", () => {
  beforeEach(() => {
    resetGardenAutomationCacheForTests();
  });

  it("reuses parsed blueprints across repeated loads while refreshing run status", async () => {
    const invoke = vi.fn(async (command: string, args?: { path?: string }) => {
      if (command === "automation_list_blueprints") {
        return {
          blueprints: [
            { id: "w1", path: "<absolute-workspace-path>/library/automations/build.md" },
            { id: "w2", path: "<absolute-workspace-path>/library/automations/ship.md" },
          ],
          truncated: false,
          next_offset: null,
        };
      }
      if (command === "automation_parse") {
        return {
          blueprint: {
            id: args?.path?.includes("ship.md") ? "w2" : "w1",
            name: args?.path?.includes("ship.md") ? "Ship" : "Build",
            nodes: args?.path?.includes("ship.md") ? [{ id: "ship" }] : [{ id: "build" }, { id: "test" }],
          },
        };
      }
      if (command === "automation_list_runs") {
        const runs = invoke.mock.calls.filter(([calledCommand]) => calledCommand === "automation_list_runs").length === 1
          ? [run({ blueprint_id: "w1", status: "completed", updated_at: "2026-06-01T00:00:00Z" })]
          : [run({ blueprint_id: "w1", status: "running", updated_at: "2026-06-02T00:00:00Z" })];
        return { runs, truncated: false, next_offset: null };
      }
      return [];
    });

    const first = await loadGardenAutomationInputs(invoke);
    const second = await loadGardenAutomationInputs(invoke);

    expect(first.automations.find((automation) => automation.id === "w1")?.runStatus).toBe("completed");
    expect(second.automations.find((automation) => automation.id === "w1")?.runStatus).toBe("running");
    expect(first.truncated).toBe(false);
    expect(invoke.mock.calls.filter(([command]) => command === "automation_parse")).toHaveLength(2);
    expect(invoke.mock.calls.filter(([command]) => command === "automation_list_runs")).toHaveLength(2);
  });
});

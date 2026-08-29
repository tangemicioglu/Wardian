import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadGardenWorkflowInputs, mergeWorkflowRunStatus, resetGardenWorkflowCacheForTests } from "./useGardenWorkflows";
import type { RunSummary } from "../workflows/run/runTypes";

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

describe("mergeWorkflowRunStatus", () => {
  it("attaches the latest run status by updated_at", () => {
    const blueprints = [{ id: "w1", name: "Build", nodeCount: 2, context: emptyContext }];
    const runs = [
      run({ status: "completed", updated_at: "2026-06-01T00:00:00Z" }),
      run({ status: "running", updated_at: "2026-06-02T00:00:00Z" }),
    ];
    expect(mergeWorkflowRunStatus(blueprints, runs)).toEqual([
      { id: "w1", label: "Build", runStatus: "running", nodeCount: 2, ...emptyInput },
    ]);
  });

  it("reports 'none' for a blueprint that has never run", () => {
    const blueprints = [{ id: "w2", name: "Ship", nodeCount: 1, context: emptyContext }];
    expect(mergeWorkflowRunStatus(blueprints, [])).toEqual([
      { id: "w2", label: "Ship", runStatus: "none", nodeCount: 1, ...emptyInput },
    ]);
  });
});

describe("loadGardenWorkflowInputs", () => {
  beforeEach(() => {
    resetGardenWorkflowCacheForTests();
  });

  it("reuses parsed blueprints across repeated loads while refreshing run status", async () => {
    const invoke = vi.fn(async (command: string, args?: { path?: string }) => {
      if (command === "workflow_list_blueprints") {
        return {
          blueprints: [
            { id: "w1", path: "<absolute-workspace-path>/library/workflows/build.md" },
            { id: "w2", path: "<absolute-workspace-path>/library/workflows/ship.md" },
          ],
          truncated: false,
          next_offset: null,
        };
      }
      if (command === "workflow_parse") {
        return {
          blueprint: {
            id: args?.path?.includes("ship.md") ? "w2" : "w1",
            name: args?.path?.includes("ship.md") ? "Ship" : "Build",
            nodes: args?.path?.includes("ship.md") ? [{ id: "ship" }] : [{ id: "build" }, { id: "test" }],
          },
        };
      }
      if (command === "workflow_list_runs") {
        const runs = invoke.mock.calls.filter(([calledCommand]) => calledCommand === "workflow_list_runs").length === 1
          ? [run({ blueprint_id: "w1", status: "completed", updated_at: "2026-06-01T00:00:00Z" })]
          : [run({ blueprint_id: "w1", status: "running", updated_at: "2026-06-02T00:00:00Z" })];
        return { runs, truncated: false, next_offset: null };
      }
      return [];
    });

    const first = await loadGardenWorkflowInputs(invoke);
    const second = await loadGardenWorkflowInputs(invoke);

    expect(first.workflows.find((workflow) => workflow.id === "w1")?.runStatus).toBe("completed");
    expect(second.workflows.find((workflow) => workflow.id === "w1")?.runStatus).toBe("running");
    expect(first.truncated).toBe(false);
    expect(invoke.mock.calls.filter(([command]) => command === "workflow_parse")).toHaveLength(2);
    expect(invoke.mock.calls.filter(([command]) => command === "workflow_list_runs")).toHaveLength(2);
  });
});

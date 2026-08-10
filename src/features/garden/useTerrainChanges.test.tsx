import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { ChangeReviewFileEntry } from "../../types";
import {
  DEFAULT_TERRAIN_BASELINE,
  terrainBaselineFor,
  useTerrainChanges,
} from "./useTerrainChanges";

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

const ROOT = "d:/work/repo";

function entry(overrides: Partial<ChangeReviewFileEntry> = {}): ChangeReviewFileEntry {
  return {
    path: "src/a.ts",
    change_kind: "modified",
    old_path: null,
    insertions: 3,
    deletions: 1,
    evidence: "attributed",
    agent_ids: ["a1"],
    turn_indices: [4],
    binary: false,
    truncated: false,
    reviewed: false,
    ...overrides,
  };
}

function response(files: ChangeReviewFileEntry[], gitAvailable = true) {
  return {
    summary: {
      schema: 1,
      baseline: "head",
      baseline_ref: "HEAD",
      from_turn_index: null,
      to_turn_index: 4,
      files,
      computed_at: "2026-08-10T00:00:00Z",
      truncated: false,
      diverged: false,
      turns_since_baseline: null,
      paths_since_baseline: files.length,
    },
    git_available: gitAvailable,
    head_ref: "abc",
    skipped_turn_records: 0,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  listenMock.mockResolvedValue(() => {});
  invokeMock.mockResolvedValue(undefined);
});

describe("terrainBaselineFor", () => {
  it("keeps a baseline the map can compute without naming an agent", () => {
    expect(terrainBaselineFor("head")).toBe("head");
    expect(terrainBaselineFor("branch_point")).toBe("branch_point");
  });

  it("falls back for the agent-scoped baselines", () => {
    // These resolve against one agent's snapshots and watermark, which is
    // meaningless for a map showing every agent at once.
    expect(terrainBaselineFor("last_effective_turn")).toBe(DEFAULT_TERRAIN_BASELINE);
    expect(terrainBaselineFor("conversation_start")).toBe(DEFAULT_TERRAIN_BASELINE);
    expect(terrainBaselineFor("unreviewed")).toBe(DEFAULT_TERRAIN_BASELINE);
  });
});

describe("useTerrainChanges", () => {
  it("loads one change set per root and names no agent", async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === "load_change_review" ? response([entry()]) : undefined,
    );

    const { result } = renderHook(() => useTerrainChanges({ enabled: true, roots: [ROOT] }));

    await waitFor(() => expect(result.current.paint.size).toBeGreaterThan(0));
    expect(invokeMock).toHaveBeenCalledWith("load_change_review", {
      request: { cwd: ROOT, baseline: "head", agent_id: null },
    });
    expect(result.current.paint.get("d:/work/repo/src/a.ts")?.kind).toBe("modified");
    expect(result.current.paint.get("d:/work/repo")?.count).toBe(1);
  });

  it("adopts the pane's baseline when the map can render it", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "load_change_review_prefs") return { schema: 1, baseline: "branch_point" };
      if (command === "load_change_review") return response([entry()]);
      return undefined;
    });

    const { result } = renderHook(() => useTerrainChanges({ enabled: true, roots: [ROOT] }));
    await waitFor(() => expect(result.current.baseline).toBe("branch_point"));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("load_change_review", {
        request: { cwd: ROOT, baseline: "branch_point", agent_id: null },
      }),
    );
  });

  it("falls back when the pane's baseline is agent-scoped", async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === "load_change_review_prefs"
        ? { schema: 1, baseline: "last_effective_turn" }
        : undefined,
    );

    const { result } = renderHook(() => useTerrainChanges({ enabled: true, roots: [ROOT] }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("load_change_review_prefs"));
    expect(result.current.baseline).toBe("head");
  });

  it("does not refetch a root it already holds", async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === "load_change_review" ? response([entry()]) : undefined,
    );

    const { result, rerender } = renderHook(
      (props: { roots: string[] }) => useTerrainChanges({ enabled: true, roots: props.roots }),
      { initialProps: { roots: [ROOT] } },
    );
    await waitFor(() => expect(result.current.paint.size).toBeGreaterThan(0));
    rerender({ roots: [ROOT] });
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(invokeMock.mock.calls.filter(([command]) => command === "load_change_review")).toHaveLength(1);
  });

  it("recomputes a root on explorer-changed rather than polling", async () => {
    type Handler = (event: { payload: unknown }) => void;
    const handlers: Record<string, Handler> = {};
    listenMock.mockImplementation(async (event: string, handler: unknown) => {
      handlers[event] = handler as Handler;
      return () => {};
    });
    invokeMock.mockImplementation(async (command: string) =>
      command === "load_change_review" ? response([entry()]) : undefined,
    );

    const { result } = renderHook(() => useTerrainChanges({ enabled: true, roots: [ROOT] }));
    await waitFor(() => expect(result.current.paint.size).toBeGreaterThan(0));

    invokeMock.mockImplementation(async (command: string) =>
      command === "load_change_review"
        ? response([entry(), entry({ path: "src/b.ts", change_kind: "added" })])
        : undefined,
    );
    await waitFor(() => expect(handlers["explorer-changed"]).toBeDefined());
    handlers["explorer-changed"]({ payload: { root_path: "D:\\work\\repo", changed_paths: [] } });

    await waitFor(() => expect(result.current.paint.get("d:/work/repo/src")?.kind).toBe("mixed"));
  });

  it("ignores a change event for a root it is not showing", async () => {
    type Handler = (event: { payload: unknown }) => void;
    const handlers: Record<string, Handler> = {};
    listenMock.mockImplementation(async (event: string, handler: unknown) => {
      handlers[event] = handler as Handler;
      return () => {};
    });
    invokeMock.mockImplementation(async (command: string) =>
      command === "load_change_review" ? response([entry()]) : undefined,
    );

    const { result } = renderHook(() => useTerrainChanges({ enabled: true, roots: [ROOT] }));
    await waitFor(() => expect(result.current.paint.size).toBeGreaterThan(0));
    const before = invokeMock.mock.calls.filter(([command]) => command === "load_change_review").length;

    handlers["explorer-changed"]({ payload: { root_path: "D:\\elsewhere", changed_paths: [] } });
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(invokeMock.mock.calls.filter(([command]) => command === "load_change_review")).toHaveLength(
      before,
    );
  });

  it("records a root without git instead of failing the map", async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === "load_change_review" ? response([], false) : undefined,
    );

    const { result } = renderHook(() => useTerrainChanges({ enabled: true, roots: [ROOT] }));
    await waitFor(() => expect(result.current.withoutGit.has(ROOT)).toBe(true));
    expect(result.current.paint.size).toBe(0);
  });

  it("degrades a failed root to unpainted ground", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "load_change_review") throw new Error("not a repository");
      return undefined;
    });

    const { result } = renderHook(() => useTerrainChanges({ enabled: true, roots: [ROOT] }));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("load_change_review", expect.anything()));
    expect(result.current.paint.size).toBe(0);
  });

  it("fetches nothing while disabled", () => {
    const { result } = renderHook(() => useTerrainChanges({ enabled: false, roots: [ROOT] }));
    expect(invokeMock).not.toHaveBeenCalled();
    expect(result.current.paint.size).toBe(0);
  });
});

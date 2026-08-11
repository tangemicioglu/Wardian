import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";

import type { ChangeReviewFileEntry } from "../../types";
import { useTerrainOpen } from "./useTerrainOpen";
import type { TerrainChangeEntry } from "./useTerrainChanges";

const navigationOpen = vi.hoisted(() => vi.fn((_request: unknown) => "surface-1"));
const pinTransient = vi.hoisted(() => vi.fn());
const openFileWithSettings = vi.hoisted(() => vi.fn(async () => "wardian" as const));

vi.mock("../../layout/AppShell", () => ({
  useAppShellWorkbenchNavigation: () => ({ open: navigationOpen, pin_transient: pinTransient }),
}));

vi.mock("../files/fileOpenRouting", () => ({ openFileWithSettings }));

vi.mock("../../store/useSettingsStore", () => ({
  useSettingsStore: (selector: (state: unknown) => unknown) =>
    selector({
      externalEditor: "system",
      externalEditorCustomExecutable: null,
      fileOpenActions: { text: "wardian", image: "external", pdf: "external" },
    }),
}));

const invokeMock = vi.mocked(invoke);

const ROOT = "d:/work/repo";
const PATH = "d:/work/repo/src/a.ts";

function entry(overrides: Partial<ChangeReviewFileEntry> = {}): ChangeReviewFileEntry {
  return {
    path: "src/a.ts",
    change_kind: "modified",
    old_path: null,
    insertions: 4,
    deletions: 1,
    evidence: "attributed",
    agent_ids: ["a1"],
    turn_indices: [7],
    binary: false,
    truncated: false,
    reviewed: false,
    ...overrides,
  };
}

function entries(overrides: Partial<ChangeReviewFileEntry> = {}) {
  const map = new Map<string, TerrainChangeEntry>();
  map.set(PATH, { entry: entry(overrides), root: ROOT, baselineRef: "abc123" });
  return map;
}

beforeEach(() => {
  vi.clearAllMocks();
  invokeMock.mockResolvedValue(undefined);
});

describe("useTerrainOpen", () => {
  it("opens a changed file with its comparison already open", async () => {
    const { result } = renderHook(() =>
      useTerrainOpen({ entries: entries(), baseline: "head" }),
    );
    await act(async () => {
      result.current(PATH);
    });

    expect(navigationOpen).toHaveBeenCalledTimes(1);
    const request = navigationOpen.mock.calls[0][0] as unknown as {
      surface_type: string;
      state: { comparison_open: boolean; comparison_baseline: Record<string, unknown> };
    };
    expect(request.surface_type).toBe("files");
    expect(request.state.comparison_open).toBe(true);
    expect(request.state.comparison_baseline).toMatchObject({
      kind: "git_revision",
      revision: "abc123",
      cwd: ROOT,
      path: "src/a.ts",
      absent: false,
    });
    expect(pinTransient).toHaveBeenCalledWith("surface-1");
  });

  it("compares a rename against the name it had at the baseline", async () => {
    const { result } = renderHook(() =>
      useTerrainOpen({
        entries: entries({ change_kind: "renamed", old_path: "src/old.ts" }),
        baseline: "head",
      }),
    );
    await act(async () => {
      result.current(PATH);
    });

    const request = navigationOpen.mock.calls[0][0] as unknown as {
      state: { comparison_baseline: { path: string } };
    };
    expect(request.state.comparison_baseline.path).toBe("src/old.ts");
  });

  it("marks an added file absent rather than pointing at a revision without it", async () => {
    const { result } = renderHook(() =>
      useTerrainOpen({ entries: entries({ change_kind: "added" }), baseline: "head" }),
    );
    await act(async () => {
      result.current(PATH);
    });

    const request = navigationOpen.mock.calls[0][0] as unknown as {
      state: { comparison_baseline: { absent: boolean } };
    };
    expect(request.state.comparison_baseline.absent).toBe(true);
  });

  it("advances the watermark for every agent that claimed the path", async () => {
    const { result } = renderHook(() =>
      useTerrainOpen({ entries: entries({ agent_ids: ["a1", "a2"] }), baseline: "head" }),
    );
    await act(async () => {
      result.current(PATH);
      await Promise.resolve();
    });

    await vi.waitFor(() => {
      const saves = invokeMock.mock.calls.filter(
        ([command]) => command === "save_change_review_watermark",
      );
      expect(saves).toHaveLength(2);
      expect(
        saves.map(([, payload]) => (payload as { watermark: { agent_id: string } }).watermark.agent_id),
      ).toEqual(["a1", "a2"]);
    });
  });

  it("advances no watermark for a write no agent claimed", async () => {
    // An inferred write belongs to nobody; inventing a claimant here would forge
    // the attribution the evidence discriminant exists to withhold.
    const { result } = renderHook(() =>
      useTerrainOpen({
        entries: entries({ evidence: "inferred", agent_ids: [] }),
        baseline: "head",
      }),
    );
    await act(async () => {
      result.current(PATH);
      await Promise.resolve();
    });

    expect(
      invokeMock.mock.calls.filter(([command]) => command === "save_change_review_watermark"),
    ).toHaveLength(0);
  });

  it("opens an unchanged file through the file-open preferences", async () => {
    const { result } = renderHook(() =>
      useTerrainOpen({ entries: new Map(), baseline: "head" }),
    );
    await act(async () => {
      result.current("d:/work/repo/src/quiet.ts");
    });

    expect(navigationOpen).not.toHaveBeenCalled();
    expect(openFileWithSettings).toHaveBeenCalledWith(
      "d:/work/repo/src/quiet.ts",
      expect.objectContaining({ external_editor: "system" }),
    );
  });

  it("routes binary content to the ordinary open rather than an empty comparison", async () => {
    const { result } = renderHook(() =>
      useTerrainOpen({ entries: entries({ binary: true }), baseline: "head" }),
    );
    await act(async () => {
      result.current(PATH);
    });

    expect(navigationOpen).not.toHaveBeenCalled();
    expect(openFileWithSettings).toHaveBeenCalled();
  });

  it("does not fail the open when recording the review fails", async () => {
    invokeMock.mockRejectedValue(new Error("disk full"));
    const { result } = renderHook(() =>
      useTerrainOpen({ entries: entries(), baseline: "head" }),
    );
    await act(async () => {
      result.current(PATH);
      await Promise.resolve();
    });

    expect(navigationOpen).toHaveBeenCalledTimes(1);
  });
});

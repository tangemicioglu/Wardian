import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConfig, ChangeReviewLoadResponse } from "../../types";
import { FileEditorControllerRegistry } from "../files/fileEditorController";
import { fileResourceClient } from "../files/fileResourceClient";
import { ChangesPanel } from "./ChangesPanel";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

type ExplorerChangedEvent = {
  payload: {
    root_path: string;
    changed_paths: string[];
  };
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

const agent = {
  session_id: "agent-1",
  session_name: "Reviewer",
  agent_class: "default",
  folder: "C:/workspace",
  is_off: false,
  provider: "mock",
} as AgentConfig;

const response: ChangeReviewLoadResponse = {
  summary: {
    schema: 1,
    baseline: "last_effective_turn",
    baseline_ref: null,
    from_turn_index: 4,
    to_turn_index: 4,
    files: [{
      path: "src/agent.ts",
      change_kind: "modified",
      old_path: null,
      insertions: 3,
      deletions: 1,
      evidence: "attributed",
      agent_ids: ["agent-1"],
      turn_indices: [4],
      binary: false,
      truncated: false,
      reviewed: false,
    }],
    computed_at: "2026-08-01T00:00:00Z",
    truncated: false,
  },
  git_available: true,
  head_ref: "head-1",
  skipped_turn_records: 0,
};

function renderPanel(visible = true, turn_revision = 1) {
  return render(
    <ChangesPanel
      visible={visible}
      agents={[agent]}
      selected_agent_ids={new Set([agent.session_id])}
      turn_revision={turn_revision}
      editor_registry={new FileEditorControllerRegistry(fileResourceClient)}
    />,
  );
}

describe("ChangesPanel", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    listenMock.mockResolvedValue(() => undefined);
    invokeMock.mockImplementation((command: string) => {
      if (command === "load_change_review_prefs") return Promise.resolve({ schema: 1, baseline: "last_effective_turn" });
      if (command === "get_explorer_root") return Promise.resolve("C:/workspace");
      if (command === "load_change_review") return Promise.resolve(response);
      return Promise.resolve(undefined);
    });
  });

  it("renders the scoped change set with attribution and the default baseline", async () => {
    renderPanel();

    expect(await screen.findByText("src/agent.ts")).toBeInTheDocument();
    expect(screen.getByText("attributed")).toBeInTheDocument();
    expect(screen.getByLabelText("Change review baseline")).toHaveValue("last_effective_turn");
    expect(screen.queryByRole("button", { name: "Refresh Changes" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Mark reviewed" })).not.toBeInTheDocument();
    expect(screen.queryByText("Review live file changes with turn attribution.")).not.toBeInTheDocument();
    expect(listenMock).toHaveBeenCalledWith("git-changed", expect.any(Function));
    expect(listenMock).toHaveBeenCalledWith("explorer-changed", expect.any(Function));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      "explorer_watch",
      { rootPath: "C:/workspace" },
    ));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      "load_change_review",
      expect.objectContaining({
        request: expect.objectContaining({
          cwd: "C:/workspace",
          baseline: "last_effective_turn",
          agent_id: "agent-1",
        }),
      }),
    ));
    expect(invokeMock).not.toHaveBeenCalledWith("git_show_file_revision", expect.anything());
  });

  it("restores the global baseline before the first change-set load", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "load_change_review_prefs") return Promise.resolve({ schema: 1, baseline: "branch_point" });
      if (command === "get_explorer_root") return Promise.resolve("C:/workspace");
      if (command === "load_change_review") return Promise.resolve(response);
      return Promise.resolve(undefined);
    });

    renderPanel();

    await waitFor(() => {
      expect(screen.getByLabelText("Change review baseline")).toHaveValue("branch_point");
      const loads = invokeMock.mock.calls.filter(([command]) => command === "load_change_review");
      expect(loads).toHaveLength(1);
      expect(loads[0][1]).toEqual(expect.objectContaining({
        request: expect.objectContaining({ baseline: "branch_point" }),
      }));
    });
  });

  it("saves only an operator baseline change", async () => {
    renderPanel();

    await screen.findByText("src/agent.ts");
    expect(invokeMock).not.toHaveBeenCalledWith("save_change_review_prefs", expect.anything());

    fireEvent.change(screen.getByLabelText("Change review baseline"), { target: { value: "head" } });

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      "save_change_review_prefs",
      { prefs: { schema: 1, baseline: "head" } },
    ));
    const saves = invokeMock.mock.calls.filter(([command]) => command === "save_change_review_prefs");
    expect(saves).toHaveLength(1);
  });

  it("does not compute while hidden and recomputes when the sidebar becomes visible", async () => {
    const view = renderPanel(false);

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      "get_explorer_root",
      { sessionId: "agent-1" },
    ));
    expect(invokeMock).not.toHaveBeenCalledWith("load_change_review", expect.anything());

    view.rerender(
      <ChangesPanel
        visible
        agents={[agent]}
        selected_agent_ids={new Set([agent.session_id])}
        turn_revision={1}
        editor_registry={new FileEditorControllerRegistry(fileResourceClient)}
      />,
    );

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      "load_change_review",
      expect.anything(),
    ));
  });

  it("recomputes at a completed-turn revision", async () => {
    const view = renderPanel(true, 1);
    await screen.findByText("src/agent.ts");
    const initialLoads = invokeMock.mock.calls.filter(([command]) => command === "load_change_review").length;

    view.rerender(
      <ChangesPanel
        visible
        agents={[agent]}
        selected_agent_ids={new Set([agent.session_id])}
        turn_revision={2}
        editor_registry={new FileEditorControllerRegistry(fileResourceClient)}
      />,
    );

    await waitFor(() => {
      const loads = invokeMock.mock.calls.filter(([command]) => command === "load_change_review").length;
      expect(loads).toBeGreaterThan(initialLoads);
    });
  });

  it("recomputes once for one debounced explorer change payload", async () => {
    let explorerHandler: ((event: ExplorerChangedEvent) => void) | undefined;
    listenMock.mockImplementation(async (event: string, callback: unknown) => {
      if (event === "explorer-changed") {
        explorerHandler = callback as (event: ExplorerChangedEvent) => void;
      }
      return () => undefined;
    });

    renderPanel();
    await screen.findByText("src/agent.ts");
    await waitFor(() => expect(explorerHandler).toBeDefined());
    const initialLoads = invokeMock.mock.calls.filter(([command]) => command === "load_change_review").length;

    explorerHandler?.({
      payload: {
        root_path: "C:/workspace",
        changed_paths: Array.from({ length: 50 }, (_, index) => `C:/workspace/file-${index}.ts`),
      },
    });

    await waitFor(() => {
      const loads = invokeMock.mock.calls.filter(([command]) => command === "load_change_review").length;
      expect(loads).toBe(initialLoads + 1);
    });
  });

  it("unwatches the workspace when the pane unmounts", async () => {
    const { unmount } = renderPanel();
    await screen.findByText("src/agent.ts");
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      "explorer_watch",
      { rootPath: "C:/workspace" },
    ));

    unmount();

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      "explorer_unwatch",
      { rootPath: "C:/workspace" },
    ));
  });

});

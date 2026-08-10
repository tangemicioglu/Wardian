import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConfig, ChangeReviewLoadResponse } from "../../types";
import type { WorkbenchNavigationService } from "../workbench/navigationService";
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
    diverged: false,
    turns_since_baseline: 0,
    paths_since_baseline: 1,
  },
  git_available: true,
  head_ref: "head-1",
  workspace_root: "C:/workspace",
  skipped_turn_records: 0,
};

let navigation: {
  open: ReturnType<typeof vi.fn>;
  pin_transient: ReturnType<typeof vi.fn>;
};

function navigationService(): WorkbenchNavigationService {
  return navigation as unknown as WorkbenchNavigationService;
}

function renderPanel(visible = true, turn_revision = 1) {
  return render(
    <ChangesPanel
      visible={visible}
      agents={[agent]}
      selected_agent_ids={new Set([agent.session_id])}
      turn_revision={turn_revision}
      navigation={navigationService()}
    />,
  );
}

describe("ChangesPanel", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    navigation = { open: vi.fn(() => "surface-1"), pin_transient: vi.fn() };
    listenMock.mockResolvedValue(() => undefined);
    invokeMock.mockImplementation((command: string) => {
      if (command === "load_change_review_prefs") return Promise.resolve({ schema: 1, baseline: "last_effective_turn" });
      if (command === "get_explorer_root") return Promise.resolve("C:/workspace");
      if (command === "load_change_review") return Promise.resolve(response);
      if (command === "load_change_review_watermark") return Promise.resolve(null);
      return Promise.resolve(undefined);
    });
  });

  it("renders a compact, file-first change list with accessible attribution", async () => {
    renderPanel();

    expect(await screen.findByText("agent.ts")).toBeInTheDocument();
    expect(screen.getByText("src")).toBeInTheDocument();
    expect(screen.getByText("+3")).toHaveClass("text-[var(--color-wardian-success)]");
    expect(screen.getByText("-1")).toHaveClass("text-[var(--color-wardian-error)]");
    expect(screen.getByLabelText("Modified change, attributed to an agent edit")).toHaveClass("rounded-full");
    expect(screen.queryByText("attributed")).not.toBeInTheDocument();
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

  it("keeps inferred evidence out of the row while preserving its line statistics", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "load_change_review_prefs") return Promise.resolve({ schema: 1, baseline: "last_effective_turn" });
      if (command === "get_explorer_root") return Promise.resolve("C:/workspace");
      if (command === "load_change_review") return Promise.resolve({
        ...response,
        summary: {
          ...response.summary,
          files: [{
            ...response.summary.files[0],
            evidence: "inferred" as const,
            insertions: 9,
            deletions: 7,
          }],
        },
      });
      return Promise.resolve(undefined);
    });

    renderPanel();

    expect(await screen.findByText("+9")).toHaveClass("text-[var(--color-wardian-success)]");
    expect(screen.getByText("-7")).toHaveClass("text-[var(--color-wardian-error)]");
    expect(screen.getByLabelText("Modified change, detected from workspace changes")).toBeInTheDocument();
    expect(screen.queryByText("inferred")).not.toBeInTheDocument();
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

  it("uses descriptive baseline labels without changing wire values", async () => {
    renderPanel();

    await screen.findByText("src/agent.ts");

    expect(screen.getByText("Since")).toBeInTheDocument();
    const options = [...screen.getByLabelText("Change review baseline").querySelectorAll("option")]
      .map((option) => ({ value: option.value, label: option.textContent }));
    expect(options).toEqual([
      { value: "last_effective_turn", label: "Last turn" },
      { value: "conversation_start", label: "This conversation" },
      { value: "branch_point", label: "This branch" },
      { value: "head", label: "Last commit" },
      { value: "unreviewed", label: "I last looked" },
    ]);
  });

  it("opens the diff in a workbench surface instead of rendering it inline", async () => {
    renderPanel();
    await screen.findByText("src/agent.ts");

    fireEvent.click(screen.getByRole("button", { name: /src\/agent\.ts/ }));

    expect(navigation.open).toHaveBeenCalledWith({
      surface_type: "files",
      resource_key: "file:C:/workspace/src/agent.ts",
      state: expect.objectContaining({
        resource_kind: "file",
        presentation: "editor",
        comparison_open: true,
        comparison_baseline: {
          kind: "git_revision",
          revision: "HEAD",
          cwd: "C:/workspace",
          path: "src/agent.ts",
          label: "Last turn",
          absent: false,
        },
      }),
    });
    expect(navigation.pin_transient).toHaveBeenCalledWith("surface-1");
    // The sidebar never loads diff content itself; the surface owns it.
    expect(invokeMock).not.toHaveBeenCalledWith("git_show_file_revision", expect.anything());
  });

  it("carries the selected baseline wording into the comparison", async () => {
    renderPanel();
    await screen.findByText("src/agent.ts");

    fireEvent.change(screen.getByLabelText("Change review baseline"), { target: { value: "branch_point" } });
    await waitFor(() => expect(screen.getByLabelText("Change review baseline")).toHaveValue("branch_point"));
    fireEvent.click(screen.getByRole("button", { name: /src\/agent\.ts/ }));

    expect(navigation.open).toHaveBeenCalledWith(expect.objectContaining({
      state: expect.objectContaining({
        comparison_baseline: expect.objectContaining({ label: "This branch" }),
      }),
    }));
  });

  it("marks an added file absent rather than reading a revision without it", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "load_change_review_prefs") return Promise.resolve({ schema: 1, baseline: "last_effective_turn" });
      if (command === "get_explorer_root") return Promise.resolve("C:/workspace");
      if (command === "load_change_review") return Promise.resolve({
        ...response,
        summary: {
          ...response.summary,
          baseline_ref: "abc123",
          files: [{ ...response.summary.files[0], path: "src/new.ts", change_kind: "added" as const }],
        },
      });
      if (command === "load_change_review_watermark") return Promise.resolve(null);
      return Promise.resolve(undefined);
    });

    renderPanel();
    await screen.findByText("src/new.ts");
    fireEvent.click(screen.getByRole("button", { name: /src\/new\.ts/ }));

    expect(navigation.open).toHaveBeenCalledWith(expect.objectContaining({
      state: expect.objectContaining({
        comparison_baseline: expect.objectContaining({ revision: "abc123", absent: true }),
      }),
    }));
  });

  it("compares a renamed file against the name it had at the baseline", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "load_change_review_prefs") return Promise.resolve({ schema: 1, baseline: "last_effective_turn" });
      if (command === "get_explorer_root") return Promise.resolve("C:/workspace");
      if (command === "load_change_review") return Promise.resolve({
        ...response,
        summary: {
          ...response.summary,
          files: [{
            ...response.summary.files[0],
            path: "src/renamed.ts",
            old_path: "src/original.ts",
            change_kind: "renamed" as const,
          }],
        },
      });
      if (command === "load_change_review_watermark") return Promise.resolve(null);
      return Promise.resolve(undefined);
    });

    renderPanel();
    await screen.findByText("src/renamed.ts");
    fireEvent.click(screen.getByRole("button", { name: /src\/renamed\.ts/ }));

    expect(navigation.open).toHaveBeenCalledWith(expect.objectContaining({
      resource_key: "file:C:/workspace/src/renamed.ts",
      state: expect.objectContaining({
        comparison_baseline: expect.objectContaining({ path: "src/original.ts", absent: false }),
      }),
    }));
  });

  it("writes a watermark when a file diff is opened", async () => {
    renderPanel();
    await screen.findByText("src/agent.ts");
    expect(invokeMock).not.toHaveBeenCalledWith("save_change_review_watermark", expect.anything());

    fireEvent.click(screen.getByRole("button", { name: /src\/agent\.ts/ }));

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      "save_change_review_watermark",
      {
        watermark: expect.objectContaining({
          agent_id: "agent-1",
          workspace: "C:/workspace",
          reviewed_head: "head-1",
          reviewed_paths: [{
            path: "src/agent.ts",
            change_kind: "modified",
            insertions: 3,
            deletions: 1,
          }],
        }),
      },
    ));
    const saves = invokeMock.mock.calls.filter(([command]) => command === "save_change_review_watermark");
    expect(saves).toHaveLength(1);
  });

  it("offers to re-anchor a diverged baseline without moving it silently", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "load_change_review_prefs") return Promise.resolve({ schema: 1, baseline: "conversation_start" });
      if (command === "get_explorer_root") return Promise.resolve("C:/workspace");
      if (command === "load_change_review") return Promise.resolve({
        ...response,
        summary: {
          ...response.summary,
          baseline: "conversation_start" as const,
          diverged: true,
          turns_since_baseline: 140,
          paths_since_baseline: 260,
        },
      });
      if (command === "load_change_review_watermark") return Promise.resolve(null);
      return Promise.resolve(undefined);
    });

    renderPanel();
    await screen.findByText("src/agent.ts");

    expect(await screen.findByText(/drifted 140 turns and 260 files/)).toBeInTheDocument();
    // The pin is the operator's explicit choice, so it must still be selected
    // until they act on the offer.
    expect(screen.getByLabelText("Change review baseline")).toHaveValue("conversation_start");

    fireEvent.click(screen.getByRole("button", { name: "Compare from the last turn instead" }));

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith(
      "save_change_review_prefs",
      { prefs: { schema: 1, baseline: "last_effective_turn" } },
    ));
  });

  it("shows no divergence notice for an undiverged baseline", async () => {
    renderPanel();
    await screen.findByText("src/agent.ts");

    expect(screen.queryByText(/drifted/)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Compare from the last turn instead" }),
    ).not.toBeInTheDocument();
  });

  it("reports an unavailable workbench instead of silently doing nothing", async () => {
    render(
      <ChangesPanel
        visible
        agents={[agent]}
        selected_agent_ids={new Set([agent.session_id])}
        turn_revision={1}
        navigation={null}
      />,
    );
    await screen.findByText("src/agent.ts");

    fireEvent.click(screen.getByRole("button", { name: /src\/agent\.ts/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Workbench navigation is unavailable.");
    expect(invokeMock).not.toHaveBeenCalledWith("save_change_review_watermark", expect.anything());
  });

  it("keeps skipped record counts diagnostic-only", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "load_change_review_prefs") return Promise.resolve({ schema: 1, baseline: "last_effective_turn" });
      if (command === "get_explorer_root") return Promise.resolve("C:/workspace");
      if (command === "load_change_review") return Promise.resolve({
        ...response,
        skipped_turn_records: 3201,
      });
      return Promise.resolve(undefined);
    });

    renderPanel();

    expect(await screen.findByText("src/agent.ts")).toBeInTheDocument();
    expect(screen.queryByText(/Attribution degraded/)).not.toBeInTheDocument();
    expect(screen.queryByText(/not readable/)).not.toBeInTheDocument();
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
        navigation={navigationService()}
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
        navigation={navigationService()}
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

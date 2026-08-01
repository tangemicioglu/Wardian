import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConfig, ChangeReviewLoadResponse } from "../../../types";
import { FileEditorControllerRegistry } from "../../files/fileEditorController";
import { fileResourceClient } from "../../files/fileResourceClient";
import { ChangesSurface } from "./ChangesSurface";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

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
    }],
    computed_at: "2026-08-01T00:00:00Z",
    truncated: false,
  },
  git_available: true,
  head_ref: "head-1",
};

describe("ChangesSurface", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_explorer_root") return Promise.resolve("C:/workspace");
      if (command === "load_change_review") return Promise.resolve(response);
      return Promise.resolve(undefined);
    });
  });

  it("renders the recomputed change set with attribution and the Phase 1 baseline", async () => {
    render(
      <ChangesSurface
        surface_id="changes-1"
        state={{ baseline: "last_effective_turn" }}
        visibility="visible"
        agents={[agent]}
        selected_agent_ids={new Set([agent.session_id])}
        turn_revision={1}
        editor_registry={new FileEditorControllerRegistry(fileResourceClient)}
        on_state_change={vi.fn()}
      />,
    );

    expect(await screen.findByText("src/agent.ts")).toBeInTheDocument();
    expect(screen.getByText("attributed")).toBeInTheDocument();
    expect(screen.getByLabelText("Change review baseline")).toHaveValue("last_effective_turn");
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
  });
});

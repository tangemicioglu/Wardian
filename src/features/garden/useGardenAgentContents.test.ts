import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { AgentConfig } from "../../types";
import { readGardenMemory, readGardenMemoryHistory, useGardenAgentContents, type GardenMemoryRecord } from "./useGardenAgentContents";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockedInvoke = vi.mocked(invoke);
const agent: AgentConfig = { session_id: "a", session_name: "A", agent_class: "Builder", folder: "/workspace", is_off: false };
const memory: GardenMemoryRecord = {
  memory_id: "memory-a", revision_id: "r1", revision: 1, agent_id: "a", workspace: null,
  kind: "stable", text: "Retain evidence", evidence_excerpt: "Keep sources", evidence_hash: "hash",
  status: "active", supersedes_revision_id: null, replaced_by_revision_id: null,
  created_at: "2026-09-07", updated_at: "2026-09-07", last_verified_at: "2026-09-07",
  idempotency_key: null, sources: [],
};

beforeEach(() => { mockedInvoke.mockReset(); });

describe("useGardenAgentContents", () => {
  it("reads workspace-scoped memory independently from unavailable conversations", async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "memory_list") return [memory];
      throw new Error("Archive unavailable");
    });
    const { result } = renderHook(() => useGardenAgentContents({ ...agent, git_worktree_folder: "/worktree" }));
    await waitFor(() => expect(result.current.memories.data).toEqual([memory]));
    expect(mockedInvoke).toHaveBeenCalledWith("memory_list", { agentId: "a", workspace: "/worktree" });
    expect(mockedInvoke).toHaveBeenCalledWith("list_conversations", { agent: "a", scopeAll: false });
    await waitFor(() => expect(result.current.conversations.error).toContain("Archive unavailable"));
    expect(result.current.memories.stale).toBe(false);
  });

  it("retains the last snapshot with an explicit stale state when refresh fails", async () => {
    let fail = false;
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "memory_list") {
        if (fail) throw new Error("Offline");
        return [memory];
      }
      return { schema: 1, conversations: [] };
    });
    const { result } = renderHook(() => useGardenAgentContents(agent));
    await waitFor(() => expect(result.current.memories.data).toEqual([memory]));
    fail = true;
    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.memories.error).toContain("Offline"));
    expect(result.current.memories).toMatchObject({ data: [memory], stale: true, loading: false });
    fail = false;
    act(() => result.current.refresh());
    await waitFor(() => expect(result.current.memories).toMatchObject({ stale: false, error: null, loading: false }));
  });

  it("ignores a late response after changing agents", async () => {
    let resolveOld: (records: GardenMemoryRecord[]) => void = () => undefined;
    mockedInvoke.mockImplementation(async (command, args) => {
      if (command === "memory_list") {
        if ((args as { agentId: string }).agentId === "a") return new Promise<GardenMemoryRecord[]>((resolve) => { resolveOld = resolve; });
        return [];
      }
      return { schema: 1, conversations: [] };
    });
    const { result, rerender } = renderHook(({ config }) => useGardenAgentContents(config), { initialProps: { config: agent } });
    rerender({ config: { ...agent, session_id: "b" } });
    await waitFor(() => expect(result.current.memories.data).toEqual([]));
    await act(async () => resolveOld([memory]));
    expect(result.current.memories.data).toEqual([]);
  });

  it("loads record history only through the explicit record helpers", async () => {
    mockedInvoke.mockResolvedValue(memory);
    expect(await readGardenMemory("memory-a")).toEqual(memory);
    expect(mockedInvoke).toHaveBeenLastCalledWith("memory_get", { memoryId: "memory-a" });
    mockedInvoke.mockResolvedValue([memory]);
    expect(await readGardenMemoryHistory("memory-a")).toEqual([memory]);
    expect(mockedInvoke).toHaveBeenLastCalledWith("memory_history", { memoryId: "memory-a" });
  });
});

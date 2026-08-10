import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";

import type { AgentReachResponse } from "../../types";
import { useGardenReach } from "./useGardenReach";

const invokeMock = vi.mocked(invoke);

const ROOTS = ["d:/work/app", "d:/work/papers"];

function response(agents: AgentReachResponse["agents"]): AgentReachResponse {
  return { schema: 1, agents, skipped_turn_records: 0 };
}

describe("useGardenReach", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("asks the backend once for the roots it was given", async () => {
    invokeMock.mockResolvedValue(response([{ agent_id: "m1", roots: ["d:/work/papers"] }]));
    const { result } = renderHook(() => useGardenReach(true, ROOTS));

    await waitFor(() => expect(result.current).toHaveLength(1));
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("load_agent_reach", { roots: ROOTS });
    expect(result.current[0]).toEqual({ agent_id: "m1", roots: ["d:/work/papers"] });
  });

  it("does not re-fetch when the same roots arrive in a different order", async () => {
    invokeMock.mockResolvedValue(response([]));
    const { rerender } = renderHook(({ roots }) => useGardenReach(true, roots), {
      initialProps: { roots: ROOTS },
    });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    rerender({ roots: [...ROOTS].reverse() });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
  });

  it("stays quiet while disabled, so a hidden Garden costs nothing", () => {
    renderHook(() => useGardenReach(false, ROOTS));
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("asks for nothing when the roster has no workspace roots", () => {
    renderHook(() => useGardenReach(true, []));
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("lays out as if nobody coordinated anything when the call fails", async () => {
    // A failure here must degrade the arrangement to what it was before this
    // input existed, never blank the map.
    invokeMock.mockRejectedValue(new Error("archive unreadable"));
    const { result } = renderHook(() => useGardenReach(true, ROOTS));

    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    expect(result.current).toEqual([]);
  });

  it("does not subscribe to file events, so geometry cannot move mid-session", async () => {
    // Paint is live; geometry is not. A cross-boundary write moves a district on
    // the next launch, not while someone is looking at the map.
    const { listen } = await import("@tauri-apps/api/event");
    invokeMock.mockResolvedValue(response([]));
    renderHook(() => useGardenReach(true, ROOTS));

    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    expect(vi.mocked(listen)).not.toHaveBeenCalled();
  });
});

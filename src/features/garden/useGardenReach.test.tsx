import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";

import type { AgentConfig, AgentReachResponse } from "../../types";
import { useGardenReach } from "./useGardenReach";

const invokeMock = vi.mocked(invoke);

function agent(id: string, folder: string): AgentConfig {
  return {
    session_id: id,
    session_name: id,
    agent_class: "Coder",
    folder,
    is_off: false,
  } as AgentConfig;
}

const AGENTS = [agent("a1", "D:/work/app"), agent("a2", "D:/work/papers")];
const ROOTS = ["d:/work/app", "d:/work/papers"];

function response(agents: AgentReachResponse["agents"]): AgentReachResponse {
  return { schema: 1, agents, skipped_turn_records: 0 };
}

/** A promise plus the handles to settle it, for testing cancellation ordering. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolveFn, rejectFn) => {
    resolve = resolveFn;
    reject = rejectFn;
  });
  return { promise, resolve, reject };
}

describe("useGardenReach", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("asks the backend once for the roster's roots", async () => {
    invokeMock.mockResolvedValue(response([{ agent_id: "a1", roots: ["d:/work/papers"] }]));
    const { result } = renderHook(() => useGardenReach(true, AGENTS));

    await waitFor(() => expect(result.current).toHaveLength(1));
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("load_agent_reach", { roots: ROOTS });
    expect(result.current[0]).toEqual({ agent_id: "a1", roots: ["d:/work/papers"] });
  });

  it("does not re-fetch when the surface is hidden and shown again", async () => {
    // The once-per-session rule was a claim the code did not keep: the effect
    // depends on `enabled`, so re-showing the Garden re-read the archive and a
    // write made while it was hidden could re-seat districts on return.
    invokeMock.mockResolvedValue(response([{ agent_id: "a1", roots: ["d:/work/papers"] }]));
    const { rerender } = renderHook(({ enabled }) => useGardenReach(enabled, AGENTS), {
      initialProps: { enabled: true },
    });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    rerender({ enabled: false });
    rerender({ enabled: true });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
  });

  it("re-asks when an agent joins a workspace that already had one", async () => {
    // The response is keyed by agent while the request is keyed by root, and the
    // two do not move together: this roster change leaves the root set
    // identical, so a root-keyed cache suppressed the fetch and the new agent's
    // history was missing until the next launch.
    invokeMock.mockResolvedValue(response([]));
    const { rerender } = renderHook(({ roster }) => useGardenReach(true, roster), {
      initialProps: { roster: AGENTS },
    });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    rerender({ roster: [...AGENTS, agent("a3", "D:/work/papers")] });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
  });

  it("applies a late answer for a roster that came back while it was in flight", async () => {
    // A -> B -> A with A still outstanding. Marking A answered up front left it
    // claimed by a torn-down effect, so nothing started and nothing applied, and
    // that roster had no reach for the rest of the session.
    const first = deferred<AgentReachResponse>();
    const other = [agent("b1", "D:/other")];
    invokeMock.mockImplementation(async (_command, args) => {
      const roots = (args as unknown as { roots?: string[] } | undefined)?.roots ?? [];
      return roots.includes("d:/other") ? response([]) : first.promise;
    });

    const { result, rerender } = renderHook(({ roster }) => useGardenReach(true, roster), {
      initialProps: { roster: AGENTS },
    });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    rerender({ roster: other });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
    rerender({ roster: AGENTS });

    first.resolve(response([{ agent_id: "a1", roots: ["d:/work/papers"] }]));
    await waitFor(() => expect(result.current).toHaveLength(1));
    // Answered by the outstanding request rather than by a duplicate read.
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it("serves a roster it has already answered without asking again", async () => {
    invokeMock.mockResolvedValue(response([{ agent_id: "a1", roots: ["d:/work/papers"] }]));
    const other = [agent("b1", "D:/other")];
    const { result, rerender } = renderHook(({ roster }) => useGardenReach(true, roster), {
      initialProps: { roster: AGENTS },
    });
    await waitFor(() => expect(result.current).toHaveLength(1));

    invokeMock.mockResolvedValue(response([]));
    rerender({ roster: other });
    await waitFor(() => expect(result.current).toHaveLength(0));

    rerender({ roster: AGENTS });
    await waitFor(() => expect(result.current).toHaveLength(1));
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it("does not cache a failed read, unlike a successful one", async () => {
    // The contrast is the test. The preceding case proves a *successful* answer
    // is served from cache on return; this one runs the identical roster
    // sequence after a failure and requires the opposite, so it fails the moment
    // failures start being cached. Retrying by toggling `enabled` would prove
    // nothing — the pre-cache hook refetched on every toggle regardless.
    invokeMock.mockRejectedValueOnce(new Error("archive unreadable"));
    const other = [agent("b1", "D:/other")];
    const { result, rerender } = renderHook(({ roster }) => useGardenReach(true, roster), {
      initialProps: { roster: AGENTS },
    });
    await waitFor(() => expect(result.current).toEqual([]));

    invokeMock.mockResolvedValue(response([]));
    rerender({ roster: other });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));

    invokeMock.mockResolvedValue(response([{ agent_id: "a1", roots: ["d:/work/papers"] }]));
    rerender({ roster: AGENTS });
    await waitFor(() => expect(result.current).toHaveLength(1));
    expect(invokeMock).toHaveBeenCalledTimes(3);
  });

  it("does not lend the previous roster's reach to a roster that has none", async () => {
    // State lags the roster by a render even on a cache hit, because effects run
    // after paint. Returning the entries alone meant the new roster laid out on
    // the old one's evidence for that render — long enough to seat a district
    // that no longer exists, and then move it again when the answer arrived.
    const pending = deferred<AgentReachResponse>();
    invokeMock.mockResolvedValueOnce(response([{ agent_id: "a1", roots: ["d:/work/papers"] }]));
    const other = [agent("b1", "D:/other")];

    const { result, rerender } = renderHook(({ roster }) => useGardenReach(true, roster), {
      initialProps: { roster: AGENTS },
    });
    await waitFor(() => expect(result.current).toHaveLength(1));

    invokeMock.mockImplementationOnce(() => pending.promise as Promise<never>);
    rerender({ roster: other });
    // Synchronous on purpose: the defect is the render itself, not the answer.
    expect(result.current).toEqual([]);

    pending.resolve(response([{ agent_id: "b1", roots: ["d:/work/app"] }]));
    await waitFor(() => expect(result.current).toHaveLength(1));
    expect(result.current[0].agent_id).toBe("b1");
  });

  it("keeps the cache bounded as rosters churn", async () => {
    // The key holds the whole roster, so an unbounded map grows one entry per
    // distinct roster for the life of the session.
    invokeMock.mockResolvedValue(response([]));
    const { rerender } = renderHook(({ roster }) => useGardenReach(true, roster), {
      initialProps: { roster: AGENTS },
    });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));

    for (let index = 0; index < 20; index += 1) {
      rerender({ roster: [agent(`x${index}`, `D:/work/w${index}`)] });
      await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(index + 2));
    }

    // The first roster was evicted, so returning to it costs a read rather than
    // being served from a cache that never forgets.
    const before = invokeMock.mock.calls.length;
    rerender({ roster: AGENTS });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(before + 1));
  });

  it("stays quiet while disabled, so a hidden Garden costs nothing", () => {
    renderHook(() => useGardenReach(false, AGENTS));
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("asks for nothing when the roster has no workspace roots", () => {
    renderHook(() => useGardenReach(true, []));
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("lays out as if nobody coordinated anything when the call fails", async () => {
    invokeMock.mockRejectedValue(new Error("archive unreadable"));
    const { result } = renderHook(() => useGardenReach(true, AGENTS));

    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    expect(result.current).toEqual([]);
  });

  it("does not subscribe to file events, so geometry cannot move mid-session", async () => {
    // Paint is live; geometry is not. A cross-boundary write moves a district on
    // the next launch, not while someone is looking at the map.
    const { listen } = await import("@tauri-apps/api/event");
    invokeMock.mockResolvedValue(response([]));
    renderHook(() => useGardenReach(true, AGENTS));

    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    expect(vi.mocked(listen)).not.toHaveBeenCalled();
  });
});

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useCoalescedRead } from "./useCoalescedRead";

/** A read whose completion the test controls. */
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

describe("useCoalescedRead", () => {
  it("joins a read already in flight instead of starting a second", async () => {
    // The case this exists for: Refresh ingests, the ingest emits
    // `telemetry-updated`, the listener reads, and then Refresh reads too. One
    // click, two identical full reads of a large store.
    const first = deferred();
    const perform = vi.fn(() => first.promise);
    const { result } = renderHook(() => useCoalescedRead("60:total_tokens", perform));

    let both!: Promise<unknown>;
    await act(async () => {
      both = Promise.all([result.current(), result.current()]);
      first.resolve();
      await both;
    });

    expect(perform).toHaveBeenCalledTimes(1);
  });

  it("starts a fresh read once the previous one has settled", async () => {
    // Coalescing must not become caching. A later read of the same question is
    // a new question about a window that has moved.
    const perform = vi.fn(() => Promise.resolve());
    const { result } = renderHook(() => useCoalescedRead("60:total_tokens", perform));

    await act(async () => {
      await result.current();
    });
    await act(async () => {
      await result.current();
    });

    expect(perform).toHaveBeenCalledTimes(2);
  });

  it("does not join a read of a different question", async () => {
    // Two windows are two answers. Joining them would let the surface show
    // figures for a window it is not displaying.
    const pending = deferred();
    const perform = vi.fn(() => pending.promise);
    const { result, rerender } = renderHook(
      ({ question }) => useCoalescedRead(question, perform),
      { initialProps: { question: "60:total_tokens" } },
    );

    let first!: Promise<unknown>;
    act(() => {
      first = result.current();
    });
    // Its own act, so the re-render is flushed before the next read is taken.
    act(() => {
      rerender({ question: "1440:total_tokens" });
    });

    await act(async () => {
      const second = result.current();
      pending.resolve();
      await Promise.all([first, second]);
    });

    expect(perform).toHaveBeenCalledTimes(2);
  });

  it("releases the slot when a read rejects", async () => {
    // A failed read must not wedge the surface into joining a promise that will
    // never be retried.
    const perform = vi
      .fn()
      .mockRejectedValueOnce(new Error("store locked"))
      .mockResolvedValueOnce(undefined);
    const { result } = renderHook(() => useCoalescedRead("60:total_tokens", perform));

    await act(async () => {
      await expect(result.current()).rejects.toThrow("store locked");
    });
    await act(async () => {
      await result.current();
    });

    expect(perform).toHaveBeenCalledTimes(2);
  });
});

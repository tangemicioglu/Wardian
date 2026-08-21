import { useCallback, useRef } from "react";

/**
 * Collapse concurrent reads of the same question into one.
 *
 * Telemetry surfaces are woken from two directions that can coincide: a backstop
 * interval, and the `telemetry-updated` event the background ingest loop emits
 * when a pass advanced a source. Both fire a full read of a large store, and
 * when they land within a few milliseconds of each other the second one answers
 * a question the first is already answering.
 *
 * A caller asking a question already in flight joins that read rather than
 * starting a second. This is exact rather than a cache with a staleness window:
 * nothing is answered from a stale result, because the read being joined has not
 * finished yet.
 *
 * **A read that must observe a write cannot use this.** After an explicit
 * ingest, an in-flight read is one that queried the store *before* the ingest
 * committed, and joining it would render pre-ingest figures. `refresh` therefore
 * calls the underlying read directly and bypasses coalescing entirely.
 *
 * A TTL cache was considered and rejected. The Dashboard is a singleton surface
 * on a 15s poll against a *trailing* window, so any TTL short enough to keep
 * that window honest expires before the next poll and never hits. The only
 * duplicate reads that actually occur are the concurrent ones, which this
 * handles without a staleness policy to get wrong.
 *
 * @param question Identifies what is being read. A change means a different
 *   answer, so reads of different questions never join.
 * @param perform The read itself.
 */
export function useCoalescedRead(
  question: string,
  perform: () => Promise<void>,
): () => Promise<void> {
  const running = useRef<{ question: string; promise: Promise<void> } | null>(null);
  const performRef = useRef(perform);
  performRef.current = perform;

  return useCallback(async () => {
    const current = running.current;
    if (current?.question === question) return current.promise;

    const promise = performRef.current().finally(() => {
      if (running.current?.promise === promise) running.current = null;
    });
    running.current = { question, promise };
    return promise;
  }, [question]);
}

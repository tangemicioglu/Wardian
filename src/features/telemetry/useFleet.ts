import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { TelemetryFleet, TelemetryRefreshReport } from "./telemetryTypes";

/**
 * How often the fleet is re-read.
 *
 * This surface is left open on the side, so its window slides continuously: even
 * with no new work, a rate computed over a trailing hour changes as that hour
 * moves. Fifteen seconds keeps the figures honest without making the bars
 * twitch, which is roughly the cadence a process viewer settles on for anything
 * lumpier than CPU.
 */
const FLEET_INTERVAL_MS = 15_000;

export interface FleetState {
  fleet: TelemetryFleet | null;
  /** True only before the first answer, so the table never blanks on reload. */
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

/**
 * Every agent's rates over one trailing window.
 *
 * The answer carries the question it answers, so a response that arrives after
 * the window or measure changed is discarded rather than shown: state lags by a
 * render, which is enough for a stale answer to be indistinguishable from a
 * fresh one.
 */
export function useFleet(
  windowMinutes: number,
  measure: string,
  enabled = true,
): FleetState {
  const [answer, setAnswer] = useState<{ request: string; fleet: TelemetryFleet } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestRef = useRef(0);
  const question = `${windowMinutes}:${measure}`;

  const read = useCallback(async () => {
    const request = (requestRef.current += 1);
    try {
      const fleet = await invoke<TelemetryFleet>("telemetry_fleet", {
        windowMinutes,
        measure,
      });
      if (requestRef.current !== request) return;
      setAnswer({ request: `${windowMinutes}:${measure}`, fleet });
      setError(null);
    } catch (cause) {
      if (requestRef.current !== request) return;
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [windowMinutes, measure]);

  const refresh = useCallback(async () => {
    try {
      await invoke<TelemetryRefreshReport>("telemetry_refresh");
    } catch (cause) {
      // A failed ingest is not a failed read: the store still holds everything
      // ingested so far, and showing that beats showing an error.
      setError(cause instanceof Error ? cause.message : String(cause));
    }
    await read();
  }, [read]);

  useEffect(() => {
    if (!enabled) return;
    void read();
    const timer = window.setInterval(() => void read(), FLEET_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [enabled, read]);

  useEffect(() => {
    if (!enabled) return;
    const subscription = listen("telemetry-updated", () => void read());
    return () => {
      void subscription.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, [enabled, read]);

  const matched = answer?.request === question ? answer.fleet : null;

  return {
    fleet: matched,
    loading: matched === null && error === null,
    error,
    refresh,
  };
}

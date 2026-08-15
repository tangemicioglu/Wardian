import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type {
  TelemetryProviderRow,
  TelemetryAgentRow,
  TelemetryDashboard,
  TelemetryGrain,
  TelemetryHorizon,
  TelemetryMeasure,
  TelemetryRefreshReport,
  HorizonWindow,
} from "./telemetryTypes";

/**
 * Backstop poll interval.
 *
 * The backend emits `telemetry-updated` when a pass advanced a source, so this
 * covers only the case where nothing advanced but the window moved — a rolling
 * horizon slides forward on its own.
 */
const REFRESH_INTERVAL_MS = 120_000;

export interface AgentTableState {
  rows: readonly TelemetryAgentRow[];
  providers: readonly TelemetryProviderRow[];
  window: HorizonWindow | null;
  /** Bucket start instants aligned to every row's `spark`. */
  buckets: readonly string[];
  /** Bucket width the sparklines were drawn at. */
  grain: TelemetryGrain | null;
  /** Shared sparkline scale, so rows are comparable to each other. */
  sparkMax: number;
  /** True only before the first answer, so the table never flashes on reload. */
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

const NO_ROWS: readonly TelemetryAgentRow[] = [];
const NO_PROVIDERS: readonly TelemetryProviderRow[] = [];
const NO_BUCKETS: readonly string[] = [];

interface Answer extends TelemetryDashboard {
  /** What this answer was asked for, so a late one can be recognised. */
  request: string;
}

/**
 * Every agent's figures for one horizon, with the selected measure bucketed.
 *
 * The answer carries the question it answers so a response arriving after the
 * horizon or measure changed is dropped rather than shown: state lags by a
 * render, which is enough for a stale answer to be indistinguishable from a
 * fresh one.
 */
export function useAgentTable(
  horizon: TelemetryHorizon,
  measure: TelemetryMeasure = "active_ms",
  enabled = true,
): AgentTableState {
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestRef = useRef(0);
  const question = `${horizon}:${measure}`;

  const read = useCallback(async () => {
    const request = (requestRef.current += 1);
    try {
      const payload = await invoke<TelemetryDashboard>("telemetry_dashboard", {
        horizon,
        measure,
      });
      if (requestRef.current !== request) return;
      setAnswer({
        ...payload,
        request: `${horizon}:${measure}`,
        rows: payload.rows ?? [],
        providers: payload.providers ?? [],
        buckets: payload.buckets ?? [],
      });
      setError(null);
    } catch (cause) {
      if (requestRef.current !== request) return;
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [horizon, measure]);

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
    const timer = window.setInterval(() => void read(), REFRESH_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [enabled, read]);

  useEffect(() => {
    if (!enabled) return;
    const subscription = listen("telemetry-updated", () => void read());
    return () => {
      void subscription.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, [enabled, read]);

  const matched = answer?.request === question ? answer : null;

  return {
    rows: matched?.rows ?? NO_ROWS,
    providers: matched?.providers ?? NO_PROVIDERS,
    window: matched?.window ?? null,
    buckets: matched?.buckets ?? NO_BUCKETS,
    grain: matched?.grain ?? null,
    sparkMax: matched?.spark_max ?? 0,
    loading: matched === null && error === null,
    error,
    refresh,
  };
}

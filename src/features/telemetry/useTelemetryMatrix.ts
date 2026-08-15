import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type {
  TelemetryDimension,
  TelemetryHorizon,
  TelemetryMatrix,
  TelemetryMeasure,
  TelemetryRefreshReport,
} from "./telemetryTypes";

/**
 * Backstop poll interval.
 *
 * The backend emits `telemetry-updated` when a pass advanced a source, so this
 * covers only the case where nothing advanced but the window moved — a rolling
 * horizon slides forward on its own, and a Dashboard left open overnight would
 * otherwise keep answering yesterday's question.
 */
const REFRESH_INTERVAL_MS = 120_000;

export interface TelemetryQuery {
  horizon: TelemetryHorizon;
  dimension: TelemetryDimension;
  measure: TelemetryMeasure;
}

export interface TelemetryMatrixState {
  matrix: TelemetryMatrix | null;
  /** True only before the first answer, so the grid never flashes on a reload. */
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

/** Every field of the query, as one comparable string. */
function queryKey(query: TelemetryQuery): string {
  return `${query.horizon}|${query.dimension}|${query.measure}`;
}

/**
 * Read one matrix from the telemetry store.
 *
 * The answer is tagged with the query it answers, and a stale response is
 * dropped rather than shown. Without that, changing measure twice quickly can
 * settle on the first response: state lags by a render, so the old answer is
 * indistinguishable from the new one by the time it arrives.
 */
export function useTelemetryMatrix(query: TelemetryQuery, enabled = true): TelemetryMatrixState {
  const [answer, setAnswer] = useState<{ key: string; matrix: TelemetryMatrix } | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Identifies the read the component currently wants, so a response that
  // arrives after the query changed can be discarded.
  const requestRef = useRef(0);
  const key = queryKey(query);
  const { horizon, dimension, measure } = query;

  const read = useCallback(async () => {
    const request = (requestRef.current += 1);
    try {
      const matrix = await invoke<TelemetryMatrix>("telemetry_matrix", {
        horizon,
        dimension,
        measure,
      });
      if (requestRef.current !== request) return;
      setAnswer({ key: `${horizon}|${dimension}|${measure}`, matrix });
      setError(null);
    } catch (cause) {
      if (requestRef.current !== request) return;
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [horizon, dimension, measure]);

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
    const subscription = listen("telemetry-updated", () => {
      void read();
    });
    return () => {
      void subscription.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, [enabled, read]);

  const matrix = answer?.key === key ? answer.matrix : null;

  return {
    matrix,
    loading: matrix === null && error === null,
    error,
    refresh,
  };
}

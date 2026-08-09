import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type WheelEvent as ReactWheelEvent,
} from "react";

import type {
  BrowserConsoleEntry,
  BrowserSessionSummary,
  BrowserViewport,
  TerminalVisibility,
} from "../../types";
import {
  attachBrowserScreencast,
  cdpModifiers,
  cdpMouseButton,
  detachBrowserScreencast,
  getBrowserSession,
  isTextKey,
  navigateBrowserSession,
  pageCoordinates,
  sendBrowserKey,
  sendBrowserPointer,
  sendBrowserWheel,
  subscribeBrowserSession,
} from "./browserSessionClient";

export interface BrowserSurfaceProps {
  /** Stable workbench presentation identity. */
  surface_id: string;
  /** The browser session id this surface presents. */
  resource_key: string;
  /** URL persisted in surface state, used to reopen after a cold restart. */
  persisted_url: string;
  visibility?: TerminalVisibility;
  /** Suppresses input, matching a mirrored terminal presentation. */
  read_only?: boolean;
  on_url_change?: (surface_id: string, url: string) => void;
  on_reopen?: (url: string) => void;
  on_close_surface?: () => void;
}

/** One renderer identity per workbench presentation of a browser runtime. */
export function browserPresentationId(surfaceId: string, browserId: string): string {
  return `${surfaceId}:browser:${browserId}`;
}

/** Human label for the load state chip. */
export function loadStateLabel(summary: BrowserSessionSummary | null): string {
  if (!summary) return "Connecting";
  switch (summary.load_state) {
    case "loading":
      return "Loading";
    case "complete":
      return "Ready";
    case "failed":
      return "Failed";
    default:
      return "Idle";
  }
}

/**
 * Runtime-backed browser presentation.
 *
 * Like the agent session surface, this owns presentation state only. Closing
 * the tab detaches the screencast; it never stops the browser, which the
 * backend owns and `wardian browser close` ends.
 */
export function BrowserSurface({
  surface_id,
  resource_key,
  persisted_url,
  visibility = "visible",
  read_only = false,
  on_url_change,
  on_reopen,
  on_close_surface,
}: BrowserSurfaceProps) {
  const presentationId = browserPresentationId(surface_id, resource_key);
  const [summary, setSummary] = useState<BrowserSessionSummary | null>(null);
  const [frame, setFrame] = useState<{ data: string; width: number; height: number } | null>(null);
  const [consoleErrors, setConsoleErrors] = useState<BrowserConsoleEntry[]>([]);
  const [missing, setMissing] = useState(false);
  /** False until the session has been confirmed to exist at least once. */
  const [resolved, setResolved] = useState(false);
  const [closedReason, setClosedReason] = useState<string | null>(null);
  const [addressDraft, setAddressDraft] = useState(persisted_url);
  const [addressFocused, setAddressFocused] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const imageRef = useRef<HTMLImageElement>(null);

  const viewport: BrowserViewport = useMemo(
    () => summary?.viewport ?? { width: frame?.width ?? 1280, height: frame?.height ?? 800 },
    [frame?.height, frame?.width, summary?.viewport],
  );

  useEffect(() => {
    let cancelled = false;
    void getBrowserSession(resource_key)
      .then((found) => {
        if (cancelled) return;
        setMissing(found === null);
        setResolved(found !== null);
        if (found) setSummary(found);
      })
      .catch(() => {
        if (!cancelled) setMissing(true);
      });
    return () => {
      cancelled = true;
    };
  }, [resource_key]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void subscribeBrowserSession(resource_key, (event) => {
      switch (event.kind) {
        case "frame":
          setFrame({ data: event.data, width: event.width, height: event.height });
          break;
        case "state":
          setSummary(event.summary);
          setMissing(false);
          setResolved(true);
          break;
        case "console":
          if (event.entry.level === "error") {
            // Bounded so a page in an error loop cannot grow this without limit.
            setConsoleErrors((entries) => [...entries, event.entry].slice(-50));
          }
          break;
        case "closed":
          setClosedReason(event.reason);
          setMissing(true);
          break;
      }
    }).then((dispose) => {
      if (cancelled) {
        dispose();
        return;
      }
      unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [resource_key]);

  // The screencast is the expensive part of a browser surface, so a hidden
  // presentation stops streaming while the page itself keeps running.
  useEffect(() => {
    // Attaching before the session is known to exist would start a stream
    // against an id that may never resolve.
    if (!resolved || missing || visibility === "hidden") return undefined;
    let attached = false;
    void attachBrowserScreencast(resource_key)
      .then(() => {
        attached = true;
      })
      .catch(() => {
        /* A closed session reports itself through the `closed` event. */
      });
    return () => {
      if (attached) void detachBrowserScreencast(resource_key).catch(() => {});
    };
  }, [missing, resolved, resource_key, visibility]);

  useEffect(() => {
    const url = summary?.url;
    if (!url) return;
    if (!addressFocused) setAddressDraft(url);
    on_url_change?.(surface_id, url);
  }, [addressFocused, on_url_change, summary?.url, surface_id]);

  const runNavigation = useCallback(
    (action: string) => {
      setActionError(null);
      void navigateBrowserSession(resource_key, action).catch((error: unknown) => {
        setActionError(error instanceof Error ? error.message : String(error));
      });
    },
    [resource_key],
  );

  const toPageCoordinates = useCallback(
    (clientX: number, clientY: number) => {
      const rect = imageRef.current?.getBoundingClientRect();
      if (!rect) return null;
      return pageCoordinates(clientX, clientY, rect, viewport);
    },
    [viewport],
  );

  const handlePointer = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>, eventType: "mousePressed" | "mouseReleased" | "mouseMoved") => {
      if (read_only || missing) return;
      const point = toPageCoordinates(event.clientX, event.clientY);
      if (!point) return;
      if (eventType === "mousePressed") viewportRef.current?.focus();
      void sendBrowserPointer({
        browser_id: resource_key,
        event_type: eventType,
        x: point.x,
        y: point.y,
        button: cdpMouseButton(event.button),
        click_count: eventType === "mouseMoved" ? 0 : event.detail || 1,
        modifiers: cdpModifiers(event),
      }).catch(() => {});
    },
    [missing, read_only, resource_key, toPageCoordinates],
  );

  const handleWheel = useCallback(
    (event: ReactWheelEvent<HTMLDivElement>) => {
      if (read_only || missing) return;
      const point = toPageCoordinates(event.clientX, event.clientY);
      if (!point) return;
      void sendBrowserWheel({
        browser_id: resource_key,
        x: point.x,
        y: point.y,
        delta_x: event.deltaX,
        delta_y: event.deltaY,
        modifiers: cdpModifiers(event),
      }).catch(() => {});
    },
    [missing, read_only, resource_key, toPageCoordinates],
  );

  const handleKey = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>, eventType: "keyDown" | "keyUp") => {
      if (read_only || missing) return;
      // Leave the workbench's own chords alone so a focused page cannot
      // swallow tab switching or the command palette.
      if (event.ctrlKey || event.metaKey) return;
      event.preventDefault();
      const text = eventType === "keyDown" && isTextKey(event.key) ? event.key : undefined;
      void sendBrowserKey({
        browser_id: resource_key,
        event_type: eventType,
        key: event.key,
        code: event.code,
        ...(text ? { text } : {}),
        modifiers: cdpModifiers(event),
      }).catch(() => {});
    },
    [missing, read_only, resource_key],
  );

  if (missing) {
    return (
      <section
        className="flex h-full min-h-0 min-w-0 items-center justify-center bg-[var(--color-wardian-bg)] p-6"
        data-missing-session="true"
        data-resource-key={resource_key}
        data-surface-id={surface_id}
        data-testid="browser-surface"
      >
        <div className="max-w-md rounded-lg border border-wardian-border bg-[var(--color-wardian-card)] p-5 text-center shadow-sm">
          <h2 className="text-base font-semibold text-primary">Browser session unavailable</h2>
          <p className="mt-2 text-sm text-muted-neutral">
            {closedReason
              ? `This session ended: ${closedReason}.`
              : "This surface references a browser session that is no longer running."}
          </p>
          <div className="mt-4 flex justify-center gap-2">
            {on_reopen ? (
              <button
                className="rounded border border-wardian-border px-3 py-1.5 text-sm text-primary transition-colors hover:bg-[var(--color-wardian-card-bg-muted)]"
                onClick={() => on_reopen(addressDraft || persisted_url)}
                type="button"
              >
                Reopen this page
              </button>
            ) : null}
            {on_close_surface ? (
              <button
                className="rounded border border-wardian-border px-3 py-1.5 text-sm text-primary"
                onClick={on_close_surface}
                type="button"
              >
                Close
              </button>
            ) : null}
          </div>
        </div>
      </section>
    );
  }

  return (
    <section
      className="flex h-full min-h-0 min-w-0 flex-col bg-[var(--color-wardian-bg)]"
      data-presentation-id={presentationId}
      data-resource-key={resource_key}
      data-surface-id={surface_id}
      data-testid="browser-surface"
    >
      <header className="flex min-h-9 shrink-0 items-center gap-1.5 border-b border-wardian-border bg-[var(--color-wardian-sidebar-secondary)] px-2 py-1.5">
        <button
          aria-label="Go back"
          className="rounded px-2 py-1 text-sm text-primary transition-colors hover:bg-[var(--color-wardian-card-bg-muted)]"
          onClick={() => runNavigation("back")}
          type="button"
        >
          ←
        </button>
        <button
          aria-label="Go forward"
          className="rounded px-2 py-1 text-sm text-primary transition-colors hover:bg-[var(--color-wardian-card-bg-muted)]"
          onClick={() => runNavigation("forward")}
          type="button"
        >
          →
        </button>
        <button
          aria-label={summary?.load_state === "loading" ? "Stop loading" : "Reload"}
          className="rounded px-2 py-1 text-sm text-primary transition-colors hover:bg-[var(--color-wardian-card-bg-muted)]"
          onClick={() => runNavigation(summary?.load_state === "loading" ? "stop" : "reload")}
          type="button"
        >
          {summary?.load_state === "loading" ? "×" : "⟳"}
        </button>
        <form
          className="min-w-0 flex-1"
          onSubmit={(event) => {
            event.preventDefault();
            const target = addressDraft.trim();
            if (target) runNavigation(target);
          }}
        >
          <input
            aria-label="Address"
            className="w-full rounded border border-wardian-border bg-[var(--color-wardian-card)] px-2 py-1 text-xs text-primary"
            data-testid="browser-surface-address"
            onBlur={() => setAddressFocused(false)}
            onChange={(event) => setAddressDraft(event.target.value)}
            onFocus={() => setAddressFocused(true)}
            spellCheck={false}
            value={addressDraft}
          />
        </form>
        <div className="flex shrink-0 items-center gap-1.5">
          <span
            className="rounded-full border border-wardian-border bg-[var(--color-wardian-card)] px-2 py-0.5 text-[10px] font-medium text-muted-neutral"
            data-testid="browser-surface-load-state"
          >
            {loadStateLabel(summary)}
          </span>
          {summary ? (
            <span
              className="rounded-full border border-wardian-border bg-[var(--color-wardian-card)] px-2 py-0.5 text-[10px] font-medium text-muted-neutral"
              data-testid="browser-surface-short-ref"
            >
              {summary.short_ref}
            </span>
          ) : null}
          {read_only ? (
            <span
              className="rounded-full border border-wardian-border bg-[var(--color-wardian-card)] px-2 py-0.5 text-[10px] font-medium text-muted-neutral"
              data-testid="browser-surface-read-only"
            >
              Read only
            </span>
          ) : null}
        </div>
      </header>
      <div
        className="relative min-h-0 min-w-0 flex-1 overflow-hidden bg-[var(--color-wardian-card-bg-muted)] outline-none"
        data-testid="browser-surface-viewport"
        onKeyDown={(event) => handleKey(event, "keyDown")}
        onKeyUp={(event) => handleKey(event, "keyUp")}
        onPointerDown={(event) => handlePointer(event, "mousePressed")}
        onPointerMove={(event) => handlePointer(event, "mouseMoved")}
        onPointerUp={(event) => handlePointer(event, "mouseReleased")}
        onWheel={handleWheel}
        ref={viewportRef}
        role="application"
        aria-label={summary?.title || "Browser page"}
        tabIndex={read_only ? -1 : 0}
      >
        {frame ? (
          <img
            alt={summary?.title || "Browser page"}
            className="pointer-events-none h-full w-full select-none object-contain"
            data-testid="browser-surface-frame"
            draggable={false}
            ref={imageRef}
            src={`data:image/jpeg;base64,${frame.data}`}
          />
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-muted-neutral">
            Waiting for the first frame…
          </div>
        )}
      </div>
      <footer className="flex min-h-7 shrink-0 items-center gap-3 border-t border-wardian-border bg-[var(--color-wardian-sidebar-secondary)] px-3 py-1 text-[11px] text-muted-neutral">
        <span data-testid="browser-surface-viewport-size">
          {viewport.width}×{viewport.height}
        </span>
        {consoleErrors.length > 0 ? (
          <span
            data-testid="browser-surface-console-errors"
            title={consoleErrors[consoleErrors.length - 1]?.text}
          >
            {consoleErrors.length} console error{consoleErrors.length === 1 ? "" : "s"}
          </span>
        ) : null}
        {actionError ? (
          <span className="truncate text-[var(--color-wardian-status-error)]" data-testid="browser-surface-error">
            {actionError}
          </span>
        ) : null}
      </footer>
    </section>
  );
}

/**
 * Browser surface wire types.
 *
 * These mirror `crates/wardian-core/src/browser.rs`, which is the single
 * definition the app, the control plane, and `wardian browser` all share.
 */

export type BrowserEngineKind = "edge" | "chrome" | "chromium" | "brave" | "custom";

export type BrowserLoadState = "idle" | "loading" | "complete" | "failed";

export type BrowserViewport = {
  width: number;
  height: number;
};

export type BrowserConsoleEntry = {
  level: string;
  text: string;
};

export type BrowserSessionSummary = {
  browser_id: string;
  /** Short ref an agent or human can type, e.g. `browser:3`. */
  short_ref: string;
  url: string;
  title: string;
  load_state: BrowserLoadState;
  viewport: BrowserViewport;
  engine: BrowserEngineKind;
  owner_agent_id?: string;
  workspace?: string;
  console_error_count: number;
};

/**
 * What attaching a screencast hands back.
 *
 * The token is the credential for every later mutation. A presentation id
 * would not do: it is derived from surface and session ids, so any caller
 * could guess it.
 */
export type BrowserScreencastAttachment = {
  token: string;
  can_drive: boolean;
};

export type BrowserEngineStatus = {
  available: boolean;
  engine?: string;
  path?: string;
  /** Present when unavailable; names the fix. */
  detail?: string;
};

/**
 * Events the backend publishes for a session.
 *
 * `frame` carries a base64 JPEG rather than a blob URL so the renderer stays a
 * plain `<img>` with no object-URL lifetime to manage.
 */
export type BrowserSessionEvent =
  | { kind: "frame"; browser_id: string; data: string; width: number; height: number }
  | { kind: "state"; browser_id: string; summary: BrowserSessionSummary }
  | { kind: "console"; browser_id: string; entry: BrowserConsoleEntry }
  | { kind: "closed"; browser_id: string; reason: string };

/**
 * Persisted browser surface state.
 *
 * The runtime session id lives in the surface's `resource_key`; this holds only
 * what is needed to reopen the same page after the runtime is gone.
 */
export type BrowserSurfaceState = {
  url: string;
  viewport: BrowserViewport | null;
};

export type BrowserNavigateAction = "back" | "forward" | "reload" | "stop" | (string & {});

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  BrowserEngineStatus,
  BrowserNavigateAction,
  BrowserScreencastAttachment,
  BrowserSessionEvent,
  BrowserSessionSummary,
} from "../../types";

/** Event the backend emits when a CLI `browser open` wants a surface. */
export const BROWSER_SURFACE_OPEN_EVENT = "browser-surface-open";
/** Event carrying frames, state, console, and closure for every session. */
export const BROWSER_SESSION_EVENT = "browser-session-event";

/** Modifier bitmask the DevTools Protocol expects on input events. */
export const CDP_MODIFIER_ALT = 1;
export const CDP_MODIFIER_CTRL = 2;
export const CDP_MODIFIER_META = 4;
export const CDP_MODIFIER_SHIFT = 8;

type ModifierSource = {
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
};

/** Packs a DOM event's modifier flags into the protocol's bitmask. */
export function cdpModifiers(event: ModifierSource): number {
  return (
    (event.altKey ? CDP_MODIFIER_ALT : 0)
    | (event.ctrlKey ? CDP_MODIFIER_CTRL : 0)
    | (event.metaKey ? CDP_MODIFIER_META : 0)
    | (event.shiftKey ? CDP_MODIFIER_SHIFT : 0)
  );
}

/** Maps a DOM mouse button number to the protocol's button name. */
export function cdpMouseButton(button: number): string {
  switch (button) {
    case 1:
      return "middle";
    case 2:
      return "right";
    case 3:
      return "back";
    case 4:
      return "forward";
    default:
      return "left";
  }
}

/**
 * Converts a pointer position inside the rendered image into page coordinates.
 *
 * The screencast image is letterboxed to fit its container, so a raw client
 * offset would land in the wrong place on any non-matching aspect ratio.
 */
export function pageCoordinates(
  clientX: number,
  clientY: number,
  rect: { left: number; top: number; width: number; height: number },
  viewport: { width: number; height: number },
): { x: number; y: number } | null {
  if (rect.width <= 0 || rect.height <= 0 || viewport.width <= 0 || viewport.height <= 0) {
    return null;
  }
  const scale = Math.min(rect.width / viewport.width, rect.height / viewport.height);
  const renderedWidth = viewport.width * scale;
  const renderedHeight = viewport.height * scale;
  const offsetX = clientX - rect.left - (rect.width - renderedWidth) / 2;
  const offsetY = clientY - rect.top - (rect.height - renderedHeight) / 2;
  if (offsetX < 0 || offsetY < 0 || offsetX > renderedWidth || offsetY > renderedHeight) {
    return null;
  }
  return { x: offsetX / scale, y: offsetY / scale };
}

/** Keys that must be sent as key events rather than inserted as text. */
const NON_TEXT_KEYS = new Set([
  "Enter",
  "Tab",
  "Backspace",
  "Delete",
  "Escape",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);

/** True when a key press should carry printable text alongside the key event. */
export function isTextKey(key: string): boolean {
  return key.length === 1 && !NON_TEXT_KEYS.has(key);
}

export function browserEngineStatus(): Promise<BrowserEngineStatus> {
  return invoke<BrowserEngineStatus>("browser_engine_status");
}

export function openBrowserSession(options: {
  url?: string;
  width?: number;
  height?: number;
}): Promise<BrowserSessionSummary> {
  return invoke<BrowserSessionSummary>("open_browser_session", {
    url: options.url ?? null,
    width: options.width ?? null,
    height: options.height ?? null,
  });
}

export function listBrowserSessions(): Promise<BrowserSessionSummary[]> {
  return invoke<BrowserSessionSummary[]>("list_browser_sessions");
}

export function getBrowserSession(browserId: string): Promise<BrowserSessionSummary | null> {
  return invoke<BrowserSessionSummary | null>("get_browser_session", { browserId });
}

export function closeBrowserSession(browserId: string): Promise<void> {
  return invoke<void>("close_browser_session", { browserId });
}

export function navigateBrowserSession(
  browserId: string,
  action: BrowserNavigateAction,
  leaseToken: string,
): Promise<BrowserSessionSummary> {
  return invoke<BrowserSessionSummary>("navigate_browser_session", {
    browserId,
    action,
    leaseToken,
  });
}

/**
 * Starts streaming and reports whether this presentation may drive the page.
 *
 * The first presentation to attach holds the drive lease; later ones mirror it
 * read-only, so two panes of one session cannot race the same page.
 */
export function attachBrowserScreencast(
  browserId: string,
  presentationId: string,
): Promise<BrowserScreencastAttachment> {
  return invoke<BrowserScreencastAttachment>("attach_browser_screencast", {
    browserId,
    presentationId,
  });
}

/** Detaches by token, so a stale cleanup cannot tear down a newer attach. */
export function detachBrowserScreencast(
  browserId: string,
  leaseToken: string,
): Promise<void> {
  return invoke<void>("detach_browser_screencast", { browserId, leaseToken });
}

export function setBrowserViewport(
  browserId: string,
  width: number,
  height: number,
  leaseToken: string,
): Promise<BrowserSessionSummary> {
  return invoke<BrowserSessionSummary>("set_browser_viewport", {
    browserId,
    width,
    height,
    leaseToken,
  });
}

export function sendBrowserPointer(request: {
  browser_id: string;
  lease_token: string;
  event_type: "mousePressed" | "mouseReleased" | "mouseMoved";
  x: number;
  y: number;
  button?: string;
  click_count?: number;
  modifiers?: number;
}): Promise<void> {
  return invoke<void>("send_browser_pointer", { request });
}

export function sendBrowserWheel(request: {
  browser_id: string;
  lease_token: string;
  x: number;
  y: number;
  delta_x: number;
  delta_y: number;
  modifiers?: number;
}): Promise<void> {
  return invoke<void>("send_browser_wheel", { request });
}

export function sendBrowserKey(request: {
  browser_id: string;
  lease_token: string;
  event_type: "keyDown" | "keyUp" | "char";
  key: string;
  code?: string;
  text?: string;
  modifiers?: number;
}): Promise<void> {
  return invoke<void>("send_browser_key", { request });
}

/**
 * Subscribes to one session's events.
 *
 * Filtering by id here rather than in every caller keeps a surface from
 * re-rendering on another session's frames, which is the dominant event volume.
 */
export function subscribeBrowserSession(
  browserId: string,
  handler: (event: BrowserSessionEvent) => void,
): Promise<UnlistenFn> {
  return listen<BrowserSessionEvent>(BROWSER_SESSION_EVENT, ({ payload }) => {
    if (payload.browser_id !== browserId) return;
    handler(payload);
  });
}

/** Subscribes to CLI-originated requests to surface a new session. */
export function subscribeBrowserSurfaceOpen(
  handler: (summary: BrowserSessionSummary) => void,
): Promise<UnlistenFn> {
  return listen<BrowserSessionSummary>(BROWSER_SURFACE_OPEN_EVENT, ({ payload }) => {
    handler(payload);
  });
}

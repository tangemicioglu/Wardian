import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserSessionSummary } from "../../types";
import {
  CDP_MODIFIER_ALT,
  CDP_MODIFIER_CTRL,
  CDP_MODIFIER_META,
  CDP_MODIFIER_SHIFT,
  cdpModifiers,
  cdpMouseButton,
  isTextKey,
  openBrowserSession,
  pageCoordinates,
  reopenBrowserSurfaceSession,
  subscribeToBrowserSurfaceOpens,
} from "./browserSessionClient";

const noModifiers = { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };
const invoked = vi.mocked(invoke);

describe("cdpModifiers", () => {
  it("packs each modifier into its own bit", () => {
    expect(cdpModifiers(noModifiers)).toBe(0);
    expect(cdpModifiers({ ...noModifiers, altKey: true })).toBe(CDP_MODIFIER_ALT);
    expect(cdpModifiers({ ...noModifiers, ctrlKey: true })).toBe(CDP_MODIFIER_CTRL);
    expect(cdpModifiers({ ...noModifiers, metaKey: true })).toBe(CDP_MODIFIER_META);
    expect(cdpModifiers({ ...noModifiers, shiftKey: true })).toBe(CDP_MODIFIER_SHIFT);
  });

  it("combines several modifiers without losing any", () => {
    expect(cdpModifiers({ altKey: true, ctrlKey: true, metaKey: true, shiftKey: true }))
      .toBe(CDP_MODIFIER_ALT | CDP_MODIFIER_CTRL | CDP_MODIFIER_META | CDP_MODIFIER_SHIFT);
  });
});

describe("cdpMouseButton", () => {
  it("maps DOM button numbers to protocol names", () => {
    expect(cdpMouseButton(0)).toBe("left");
    expect(cdpMouseButton(1)).toBe("middle");
    expect(cdpMouseButton(2)).toBe("right");
    expect(cdpMouseButton(3)).toBe("back");
    expect(cdpMouseButton(4)).toBe("forward");
  });

  it("treats an unknown button as the primary one", () => {
    expect(cdpMouseButton(99)).toBe("left");
  });
});

describe("isTextKey", () => {
  it("treats single printable characters as text", () => {
    expect(isTextKey("a")).toBe(true);
    expect(isTextKey(" ")).toBe(true);
    expect(isTextKey("é")).toBe(true);
  });

  it("excludes named keys that must not be inserted as text", () => {
    for (const key of ["Enter", "Tab", "Backspace", "Escape", "ArrowLeft", "Shift", "F5"]) {
      expect(isTextKey(key), key).toBe(false);
    }
  });
});

describe("pageCoordinates", () => {
  const viewport = { width: 1000, height: 500 };

  it("maps a click at the centre of a matching-ratio image to the page centre", () => {
    const rect = { left: 0, top: 0, width: 1000, height: 500 };
    expect(pageCoordinates(500, 250, rect, viewport)).toEqual({ x: 500, y: 250 });
  });

  it("accounts for the letterbox when the container is taller than the page", () => {
    // 1000x1000 container fitting a 2:1 page renders 1000x500 centred, so the
    // rendered image starts 250px down.
    const rect = { left: 0, top: 0, width: 1000, height: 1000 };
    expect(pageCoordinates(500, 500, rect, viewport)).toEqual({ x: 500, y: 250 });
    expect(pageCoordinates(0, 250, rect, viewport)).toEqual({ x: 0, y: 0 });
  });

  it("accounts for the container's own offset on the screen", () => {
    const rect = { left: 100, top: 40, width: 1000, height: 500 };
    expect(pageCoordinates(600, 290, rect, viewport)).toEqual({ x: 500, y: 250 });
  });

  it("scales a click when the image is rendered smaller than the page", () => {
    const rect = { left: 0, top: 0, width: 500, height: 250 };
    expect(pageCoordinates(250, 125, rect, viewport)).toEqual({ x: 500, y: 250 });
  });

  it("returns null for a point in the letterbox rather than clamping it", () => {
    const rect = { left: 0, top: 0, width: 1000, height: 1000 };
    expect(pageCoordinates(500, 100, rect, viewport)).toBeNull();
    expect(pageCoordinates(500, 900, rect, viewport)).toBeNull();
  });

  it("returns null when a dimension is degenerate", () => {
    expect(pageCoordinates(0, 0, { left: 0, top: 0, width: 0, height: 500 }, viewport)).toBeNull();
    expect(
      pageCoordinates(0, 0, { left: 0, top: 0, width: 100, height: 100 }, { width: 0, height: 0 }),
    ).toBeNull();
  });
});

describe("reopenBrowserSurfaceSession", () => {
  beforeEach(() => {
    invoked.mockReset();
    invoked.mockImplementation(async (command: string) => {
      if (command === "open_browser_session") {
        return { browser_id: "b-replacement", url: "https://example.com/" };
      }
      return undefined;
    });
  });

  it("leaves the session it created running once the surface takes it", async () => {
    await reopenBrowserSurfaceSession("https://example.com/", async () => "allow");

    expect(invoked).toHaveBeenCalledWith("open_browser_session", expect.anything());
    expect(invoked).not.toHaveBeenCalledWith("close_browser_session", expect.anything());
  });

  it("closes the replacement when the rebind declines", async () => {
    const reported = vi.spyOn(console, "error").mockImplementation(() => {});
    // A coordinated workbench transaction can answer `cancel`. Nothing else
    // holds the runtime, so leaving it up would orphan a live Chromium.
    await expect(
      reopenBrowserSurfaceSession("https://example.com/", async () => "cancel"),
    ).rejects.toThrow(/declined to rebind/);

    expect(invoked).toHaveBeenCalledWith("close_browser_session", {
      browserId: "b-replacement",
    });
    reported.mockRestore();
  });

  it("carries a persisted viewport into the replacement session", async () => {
    // A restore that reverted to the default size would reopen the page at a
    // width the operator never chose.
    await reopenBrowserSurfaceSession(
      "https://example.com/",
      async () => "allow",
      { width: 900, height: 600 },
    );

    expect(invoked).toHaveBeenCalledWith("open_browser_session", {
      url: "https://example.com/",
      width: 900,
      height: 600,
      workspace: null,
      blank: false,
    });
  });

  it("sends the workspace a default address should be guessed from", async () => {
    // The backend cannot read a declared port out of a workspace nobody named.
    await openBrowserSession({ workspace: "/work/app" });

    expect(invoked).toHaveBeenCalledWith("open_browser_session", {
      url: null,
      width: null,
      height: null,
      workspace: "/work/app",
      blank: false,
    });
  });

  it("defaults every optional field rather than omitting it", async () => {
    // Tauri distinguishes an absent argument from a null one, and an absent
    // `blank` would fail deserialization on the command's `Option<bool>`.
    await openBrowserSession({});

    expect(invoked).toHaveBeenCalledWith("open_browser_session", {
      url: null,
      width: null,
      height: null,
      workspace: null,
      blank: false,
    });
  });

  it("closes the replacement when the rebind throws", async () => {
    const reported = vi.spyOn(console, "error").mockImplementation(() => {});

    await expect(
      reopenBrowserSurfaceSession("https://example.com/", async () => {
        throw new Error("workbench command rejected");
      }),
    ).rejects.toThrow("workbench command rejected");

    expect(invoked).toHaveBeenCalledWith("close_browser_session", {
      browserId: "b-replacement",
    });
    expect(reported).toHaveBeenCalled();
    reported.mockRestore();
  });

  it("reports a failed open without trying to close a session it never made", async () => {
    const reported = vi.spyOn(console, "error").mockImplementation(() => {});
    invoked.mockRejectedValueOnce(new Error("no browser engine"));

    await expect(
      reopenBrowserSurfaceSession("https://example.com/", async () => "allow"),
    ).rejects.toThrow("no browser engine");

    expect(invoked).not.toHaveBeenCalledWith("close_browser_session", expect.anything());
    expect(reported).toHaveBeenCalled();
    reported.mockRestore();
  });
});

describe("subscribeToBrowserSurfaceOpens", () => {
  let calls: string[] = [];
  let deliver: ((summary: BrowserSessionSummary) => void) | null = null;
  let pending: BrowserSessionSummary[] = [];

  function sessionSummary(browserId: string): BrowserSessionSummary {
    return {
      browser_id: browserId,
      short_ref: "browser:1",
      url: "https://example.com/",
      title: "Example",
      load_state: "complete",
      viewport: { width: 1000, height: 500 },
      engine: "edge",
      console_error_count: 0,
      network_failure_count: 0,
      popup: false,
    };
  }

  beforeEach(() => {
    calls = [];
    deliver = null;
    pending = [];
    vi.mocked(listen).mockImplementation(async (_event, handler) => {
      calls.push("listen");
      deliver = (summary) => handler({ payload: summary } as never);
      return () => {
        calls.push("unlisten");
        deliver = null;
      };
    });
    invoked.mockReset();
    invoked.mockImplementation(async (command: string, args?: unknown) => {
      calls.push(command);
      if (command === "pending_browser_surface_opens") return pending;
      if (command === "ack_browser_surface_open") {
        const { browserId } = args as { browserId: string };
        pending = pending.filter((summary) => summary.browser_id !== browserId);
      }
      return undefined;
    });
  });

  it("acknowledges a live open so no later reader repeats it", async () => {
    const opened: string[] = [];
    subscribeToBrowserSurfaceOpens((summary) => opened.push(summary.browser_id));
    await vi.waitFor(() => expect(calls).toContain("pending_browser_surface_opens"));

    deliver?.(sessionSummary("b-live"));

    expect(opened).toEqual(["b-live"]);
    await vi.waitFor(() => expect(calls).toContain("ack_browser_surface_open"));
  });

  it("surfaces an open that was still outstanding at mount", async () => {
    pending = [sessionSummary("b-outstanding")];
    const opened: string[] = [];
    subscribeToBrowserSurfaceOpens((summary) => opened.push(summary.browser_id));

    await vi.waitFor(() => expect(opened).toEqual(["b-outstanding"]));
    await vi.waitFor(() => expect(pending).toHaveLength(0));
  });

  it("leaves an open outstanding when disposal beats delivery", async () => {
    // Nothing here depends on a message arriving before teardown: an open the
    // frontend never surfaced stays queued for whoever mounts next.
    const opened: string[] = [];
    const dispose = subscribeToBrowserSurfaceOpens((summary) => opened.push(summary.browser_id));
    await vi.waitFor(() => expect(calls).toContain("pending_browser_surface_opens"));

    const late = deliver;
    dispose();
    pending = [sessionSummary("b-late")];
    late?.(sessionSummary("b-late"));

    expect(opened).toEqual([]);
    expect(calls).not.toContain("ack_browser_surface_open");
    expect(pending).toHaveLength(1);
  });

  it("keeps an open outstanding when reading it fails", async () => {
    invoked.mockImplementation(async (command: string) => {
      calls.push(command);
      if (command === "pending_browser_surface_opens") {
        throw new Error("ipc unavailable");
      }
      return undefined;
    });
    const opened: string[] = [];

    subscribeToBrowserSurfaceOpens((summary) => opened.push(summary.browser_id));
    await vi.waitFor(() => expect(calls).toContain("pending_browser_surface_opens"));

    // A failed read is not a lost open: it was never acknowledged, so the next
    // mount reads it again.
    expect(opened).toEqual([]);
    expect(calls).not.toContain("ack_browser_surface_open");
  });
});

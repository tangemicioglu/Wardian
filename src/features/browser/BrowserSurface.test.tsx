import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserSessionEvent, BrowserSessionSummary } from "../../types";
import { BrowserSurface, browserPresentationId, loadStateLabel } from "./BrowserSurface";

const mocks = vi.hoisted(() => ({
  getBrowserSession: vi.fn(),
  attachBrowserScreencast: vi.fn(),
  detachBrowserScreencast: vi.fn(),
  navigateBrowserSession: vi.fn(),
  sendBrowserPointer: vi.fn(),
  sendBrowserWheel: vi.fn(),
  sendBrowserKey: vi.fn(),
  subscribeBrowserSession: vi.fn(),
}));

let emit: ((event: BrowserSessionEvent) => void) | null = null;
const unlisten = vi.fn();

vi.mock("./browserSessionClient", async () => {
  const actual = await vi.importActual<typeof import("./browserSessionClient")>(
    "./browserSessionClient",
  );
  return {
    ...actual,
    getBrowserSession: mocks.getBrowserSession,
    attachBrowserScreencast: mocks.attachBrowserScreencast,
    detachBrowserScreencast: mocks.detachBrowserScreencast,
    navigateBrowserSession: mocks.navigateBrowserSession,
    sendBrowserPointer: mocks.sendBrowserPointer,
    sendBrowserWheel: mocks.sendBrowserWheel,
    sendBrowserKey: mocks.sendBrowserKey,
    subscribeBrowserSession: mocks.subscribeBrowserSession,
  };
});

function summary(overrides: Partial<BrowserSessionSummary> = {}): BrowserSessionSummary {
  return {
    browser_id: "b-1",
    short_ref: "browser:1",
    url: "https://example.com/",
    title: "Example",
    load_state: "complete",
    viewport: { width: 1000, height: 500 },
    engine: "edge",
    console_error_count: 0,
    ...overrides,
  };
}

/**
 * Renders and waits for the attach to settle.
 *
 * A surface cannot drive before its lease is granted, so a test that fired
 * input immediately would be asserting against the unattached state.
 */
async function renderSurface(
  props: Partial<Parameters<typeof BrowserSurface>[0]> = {},
  options: { expectAttach?: boolean } = {},
) {
  const rendered = render(
    <BrowserSurface
      surface_id="surface-1"
      resource_key="b-1"
      persisted_url="https://example.com/"
      {...props}
    />,
  );
  await waitFor(() => expect(mocks.subscribeBrowserSession).toHaveBeenCalled());
  if (options.expectAttach !== false) {
    await waitFor(() => expect(mocks.attachBrowserScreencast).toHaveBeenCalled());
    // Flush the attach promise so the lease lands before the test acts.
    await act(async () => { await Promise.resolve(); });
  }
  return rendered;
}

/** Gives the frame `<img>` a real box so pointer mapping has something to scale against. */
function stubFrameGeometry(width = 1000, height = 500) {
  const image = screen.getByTestId("browser-surface-frame");
  vi.spyOn(image, "getBoundingClientRect").mockReturnValue({
    left: 0,
    top: 0,
    width,
    height,
    right: width,
    bottom: height,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect);
  return image;
}

beforeEach(() => {
  vi.clearAllMocks();
  emit = null;
  mocks.getBrowserSession.mockResolvedValue(summary());
  mocks.attachBrowserScreencast.mockResolvedValue({ token: "lease-1", can_drive: true });
  mocks.detachBrowserScreencast.mockResolvedValue(undefined);
  mocks.navigateBrowserSession.mockResolvedValue(summary());
  mocks.sendBrowserPointer.mockResolvedValue(undefined);
  mocks.sendBrowserWheel.mockResolvedValue(undefined);
  mocks.sendBrowserKey.mockResolvedValue(undefined);
  mocks.subscribeBrowserSession.mockImplementation(
    (_id: string, handler: (event: BrowserSessionEvent) => void) => {
      emit = handler;
      return Promise.resolve(unlisten);
    },
  );
});

describe("browserPresentationId", () => {
  it("is unique per surface so two panes of one session do not collide", () => {
    expect(browserPresentationId("s1", "b1")).toBe("s1:browser:b1");
    expect(browserPresentationId("s2", "b1")).not.toBe(browserPresentationId("s1", "b1"));
  });
});

describe("loadStateLabel", () => {
  it("reports Connecting until a summary arrives", () => {
    expect(loadStateLabel(null)).toBe("Connecting");
  });

  it("names each load state", () => {
    expect(loadStateLabel(summary({ load_state: "loading" }))).toBe("Loading");
    expect(loadStateLabel(summary({ load_state: "complete" }))).toBe("Ready");
    expect(loadStateLabel(summary({ load_state: "failed" }))).toBe("Failed");
    expect(loadStateLabel(summary({ load_state: "idle" }))).toBe("Idle");
  });
});

describe("BrowserSurface", () => {
  it("shows the session's short ref and address once it resolves", async () => {
    await renderSurface();
    await waitFor(() =>
      expect(screen.getByTestId("browser-surface-short-ref")).toHaveTextContent("browser:1"),
    );
    expect(screen.getByTestId("browser-surface-address")).toHaveValue("https://example.com/");
    expect(screen.getByTestId("browser-surface-load-state")).toHaveTextContent("Ready");
  });

  it("waits for a frame before claiming to render the page", async () => {
    await renderSurface();
    expect(screen.getByText("Waiting for the first frame…")).toBeInTheDocument();
    act(() => {
      emit?.({ kind: "frame", browser_id: "b-1", data: "AAAA", width: 1000, height: 500 });
    });
    const image = screen.getByTestId("browser-surface-frame");
    expect(image).toHaveAttribute("src", "data:image/jpeg;base64,AAAA");
  });

  it("streams only while visible so a hidden tab stops costing frames", async () => {
    const { rerender } = await renderSurface();
    await waitFor(() =>
      expect(mocks.attachBrowserScreencast).toHaveBeenCalledWith("b-1", expect.any(String)),
    );
    rerender(
      <BrowserSurface
        surface_id="surface-1"
        resource_key="b-1"
        persisted_url="https://example.com/"
        visibility="hidden"
      />,
    );
    await waitFor(() =>
      expect(mocks.detachBrowserScreencast).toHaveBeenCalledWith("b-1", "lease-1"),
    );
  });

  it("submits the address bar as a navigation", async () => {
    await renderSurface();
    const address = screen.getByTestId("browser-surface-address");
    // Focus first, the way a user reaches the address bar; focus is what keeps
    // an in-flight page update from overwriting what is being typed.
    fireEvent.focus(address);
    fireEvent.change(address, { target: { value: "localhost:5173" } });
    fireEvent.submit(address.closest("form")!);
    await waitFor(() =>
      expect(mocks.navigateBrowserSession).toHaveBeenCalledWith(
        "b-1",
        "localhost:5173",
        "lease-1",
      ),
    );
  });

  it("reloads when idle and stops when loading", async () => {
    await renderSurface();
    fireEvent.click(screen.getByLabelText("Reload"));
    await waitFor(() =>
      expect(mocks.navigateBrowserSession).toHaveBeenCalledWith("b-1", "reload", "lease-1"),
    );

    act(() => {
      emit?.({ kind: "state", browser_id: "b-1", summary: summary({ load_state: "loading" }) });
    });
    fireEvent.click(screen.getByLabelText("Stop loading"));
    await waitFor(() =>
      expect(mocks.navigateBrowserSession).toHaveBeenCalledWith("b-1", "stop", "lease-1"),
    );
  });

  it("surfaces a navigation failure rather than swallowing it", async () => {
    mocks.navigateBrowserSession.mockRejectedValue(new Error("no earlier page"));
    await renderSurface();
    fireEvent.click(screen.getByLabelText("Go back"));
    await waitFor(() =>
      expect(screen.getByTestId("browser-surface-error")).toHaveTextContent("no earlier page"),
    );
  });

  it("forwards a click in page coordinates, not container coordinates", async () => {
    await renderSurface();
    act(() => {
      emit?.({ kind: "frame", browser_id: "b-1", data: "AAAA", width: 1000, height: 500 });
    });
    stubFrameGeometry(500, 250);
    fireEvent.pointerDown(screen.getByTestId("browser-surface-viewport"), {
      clientX: 250,
      clientY: 125,
      button: 0,
      detail: 1,
    });
    await waitFor(() => expect(mocks.sendBrowserPointer).toHaveBeenCalled());
    expect(mocks.sendBrowserPointer).toHaveBeenCalledWith(
      expect.objectContaining({
        browser_id: "b-1",
        lease_token: "lease-1",
        event_type: "mousePressed",
        x: 500,
        y: 250,
      }),
    );
  });

  it("forwards a wheel event with its deltas", async () => {
    await renderSurface();
    act(() => {
      emit?.({ kind: "frame", browser_id: "b-1", data: "AAAA", width: 1000, height: 500 });
    });
    stubFrameGeometry();
    fireEvent.wheel(screen.getByTestId("browser-surface-viewport"), {
      clientX: 10,
      clientY: 10,
      deltaX: 0,
      deltaY: 120,
    });
    await waitFor(() =>
      expect(mocks.sendBrowserWheel).toHaveBeenCalledWith(
        expect.objectContaining({ delta_y: 120 }),
      ),
    );
  });

  it("sends printable keys with text and named keys without", async () => {
    await renderSurface();
    const viewport = screen.getByTestId("browser-surface-viewport");
    fireEvent.keyDown(viewport, { key: "a", code: "KeyA" });
    await waitFor(() => expect(mocks.sendBrowserKey).toHaveBeenCalled());
    expect(mocks.sendBrowserKey).toHaveBeenCalledWith(expect.objectContaining({ text: "a" }));

    mocks.sendBrowserKey.mockClear();
    fireEvent.keyDown(viewport, { key: "Enter", code: "Enter" });
    await waitFor(() => expect(mocks.sendBrowserKey).toHaveBeenCalled());
    expect(mocks.sendBrowserKey.mock.calls[0][0]).not.toHaveProperty("text");
  });

  it("leaves workbench chords to the workbench", async () => {
    await renderSurface();
    fireEvent.keyDown(screen.getByTestId("browser-surface-viewport"), {
      key: "p",
      code: "KeyP",
      ctrlKey: true,
    });
    expect(mocks.sendBrowserKey).not.toHaveBeenCalled();
  });

  it("suppresses every input path in read-only mode", async () => {
    await renderSurface({ read_only: true });
    act(() => {
      emit?.({ kind: "frame", browser_id: "b-1", data: "AAAA", width: 1000, height: 500 });
    });
    stubFrameGeometry();
    const viewport = screen.getByTestId("browser-surface-viewport");
    fireEvent.pointerDown(viewport, { clientX: 10, clientY: 10, button: 0 });
    fireEvent.wheel(viewport, { clientX: 10, clientY: 10, deltaX: 0, deltaY: 10 });
    fireEvent.keyDown(viewport, { key: "a", code: "KeyA" });
    expect(mocks.sendBrowserPointer).not.toHaveBeenCalled();
    expect(mocks.sendBrowserWheel).not.toHaveBeenCalled();
    expect(mocks.sendBrowserKey).not.toHaveBeenCalled();
    expect(screen.getByTestId("browser-surface-read-only")).toBeInTheDocument();
  });

  it("counts console errors and ignores everything else", async () => {
    await renderSurface();
    act(() => {
      emit?.({ kind: "console", browser_id: "b-1", entry: { level: "info", text: "hello" } });
      emit?.({ kind: "console", browser_id: "b-1", entry: { level: "error", text: "boom" } });
    });
    expect(screen.getByTestId("browser-surface-console-errors")).toHaveTextContent("1 console error");
  });

  it("offers a reopen path when the session is gone", async () => {
    const onReopen = vi.fn();
    await renderSurface({ on_reopen: onReopen });
    act(() => {
      emit?.({ kind: "closed", browser_id: "b-1", reason: "the browser process exited" });
    });
    expect(screen.getByText(/the browser process exited/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reopen this page" }));
    expect(onReopen).toHaveBeenCalledWith("https://example.com/");
  });

  it("mirrors read-only when another presentation holds the drive lease", async () => {
    mocks.attachBrowserScreencast.mockResolvedValue({ token: "lease-2", can_drive: false });
    await renderSurface();
    await waitFor(() =>
      expect(screen.getByTestId("browser-surface-read-only")).toBeInTheDocument(),
    );
    act(() => {
      emit?.({ kind: "frame", browser_id: "b-1", data: "AAAA", width: 1000, height: 500 });
    });
    stubFrameGeometry();

    // Every driving path must be closed, not just the viewport.
    expect(screen.getByLabelText("Go back")).toBeDisabled();
    expect(screen.getByLabelText("Reload")).toBeDisabled();
    expect(screen.getByTestId("browser-surface-address")).toBeDisabled();
    fireEvent.click(screen.getByLabelText("Go back"));
    fireEvent.pointerDown(screen.getByTestId("browser-surface-viewport"), {
      clientX: 10,
      clientY: 10,
      button: 0,
    });
    expect(mocks.navigateBrowserSession).not.toHaveBeenCalled();
    expect(mocks.sendBrowserPointer).not.toHaveBeenCalled();
  });

  it("detaches a screencast whose attach resolved after the surface was hidden", async () => {
    let resolveAttach: ((attachment: { token: string; can_drive: boolean }) => void) | null = null;
    mocks.attachBrowserScreencast.mockImplementation(
      () => new Promise((resolve) => { resolveAttach = resolve; }),
    );
    const { rerender } = await renderSurface();
    await waitFor(() => expect(mocks.attachBrowserScreencast).toHaveBeenCalled());

    rerender(
      <BrowserSurface
        surface_id="surface-1"
        resource_key="b-1"
        persisted_url="https://example.com/"
        visibility="hidden"
      />,
    );
    expect(mocks.detachBrowserScreencast).not.toHaveBeenCalled();

    // The in-flight attach lands after cleanup already ran; without the
    // cancellation check the stream would run forever for a hidden surface.
    await act(async () => {
      resolveAttach?.({ token: "late-lease", can_drive: true });
      await Promise.resolve();
    });
    await waitFor(() => expect(mocks.detachBrowserScreencast).toHaveBeenCalledTimes(1));
    // Released by its own token, so a newer attach for this surface survives.
    expect(mocks.detachBrowserScreencast).toHaveBeenCalledWith("b-1", "late-lease");
  });

  it("cannot drive until an attachment has actually granted a lease", async () => {
    mocks.attachBrowserScreencast.mockImplementation(() => new Promise(() => {}));
    await renderSurface();
    await waitFor(() => expect(mocks.attachBrowserScreencast).toHaveBeenCalled());
    // No lease has been granted, so every control is inert rather than
    // sending a mutation the backend would refuse.
    expect(screen.getByLabelText("Reload")).toBeDisabled();
    fireEvent.click(screen.getByLabelText("Reload"));
    expect(mocks.navigateBrowserSession).not.toHaveBeenCalled();
  });

  it("shows the unavailable state when the session never resolves", async () => {
    mocks.getBrowserSession.mockResolvedValue(null);
    await renderSurface({}, { expectAttach: false });
    await waitFor(() =>
      expect(screen.getByText("Browser session unavailable")).toBeInTheDocument(),
    );
    expect(mocks.attachBrowserScreencast).not.toHaveBeenCalled();
  });

  it("reports the page URL upward so the surface can persist it", async () => {
    const onUrlChange = vi.fn();
    await renderSurface({ on_url_change: onUrlChange });
    act(() => {
      emit?.({
        kind: "state",
        browser_id: "b-1",
        summary: summary({ url: "https://example.com/next" }),
      });
    });
    await waitFor(() =>
      expect(onUrlChange).toHaveBeenCalledWith("surface-1", "https://example.com/next"),
    );
  });

  it("does not overwrite the address bar while it is being edited", async () => {
    await renderSurface();
    const address = screen.getByTestId("browser-surface-address");
    fireEvent.focus(address);
    fireEvent.change(address, { target: { value: "half-typed-url" } });
    act(() => {
      emit?.({
        kind: "state",
        browser_id: "b-1",
        summary: summary({ url: "https://example.com/elsewhere" }),
      });
    });
    expect(address).toHaveValue("half-typed-url");
  });
});

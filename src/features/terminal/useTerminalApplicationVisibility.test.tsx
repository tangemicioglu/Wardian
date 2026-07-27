import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const terminalVisibility = vi.hoisted(() => ({
  set: vi.fn(() => Promise.resolve()),
}));
const nativeWindow = vi.hoisted(() => ({
  focusListener: null as ((event: { payload: boolean }) => void) | null,
  unlisten: vi.fn(),
  onFocusChanged: vi.fn(),
}));

vi.mock("./terminalSessionClient", () => ({
  setTerminalApplicationVisibility: terminalVisibility.set,
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onFocusChanged: nativeWindow.onFocusChanged }),
}));

import { useTerminalApplicationVisibility } from "./useTerminalApplicationVisibility";

describe("useTerminalApplicationVisibility", () => {
  beforeEach(() => {
    terminalVisibility.set.mockClear();
    nativeWindow.focusListener = null;
    nativeWindow.unlisten.mockClear();
    nativeWindow.onFocusChanged.mockImplementation((listener: (event: { payload: boolean }) => void) => {
      nativeWindow.focusListener = listener;
      return Promise.resolve(nativeWindow.unlisten);
    });
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
  });

  it("uses native desktop focus changes to pause and resume terminal consumption", async () => {
    const { unmount } = renderHook(() => useTerminalApplicationVisibility());

    await waitFor(() => expect(nativeWindow.focusListener).not.toBeNull());
    expect(terminalVisibility.set).toHaveBeenCalledWith(true);

    act(() => {
      nativeWindow.focusListener?.({ payload: false });
    });
    expect(terminalVisibility.set).toHaveBeenLastCalledWith(false);

    act(() => {
      nativeWindow.focusListener?.({ payload: true });
    });
    expect(terminalVisibility.set).toHaveBeenLastCalledWith(true);

    unmount();
    expect(nativeWindow.unlisten).toHaveBeenCalledTimes(1);
  });
});

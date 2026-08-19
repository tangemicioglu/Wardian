import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const terminalVisibility = vi.hoisted(() => ({
  set: vi.fn(() => Promise.resolve()),
}));
vi.mock("./terminalSessionClient", () => ({
  setTerminalApplicationVisibility: terminalVisibility.set,
}));

import { useTerminalApplicationVisibility } from "./useTerminalApplicationVisibility";

describe("useTerminalApplicationVisibility", () => {
  beforeEach(() => {
    terminalVisibility.set.mockClear();
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  });

  it("pauses terminal consumption only while the document is actually hidden", async () => {
    renderHook(() => useTerminalApplicationVisibility());
    expect(terminalVisibility.set).toHaveBeenCalledWith(true);

    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(terminalVisibility.set).toHaveBeenLastCalledWith(false);

    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await waitFor(() => expect(terminalVisibility.set).toHaveBeenLastCalledWith(true));
  });

  it("keeps terminal consumption active when a visible window loses focus", () => {
    renderHook(() => useTerminalApplicationVisibility());
    terminalVisibility.set.mockClear();

    act(() => {
      window.dispatchEvent(new Event("blur"));
    });

    expect(terminalVisibility.set).not.toHaveBeenCalled();
  });
});

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}));

const { mockImageFromPath, mockWriteImage } = vi.hoisted(() => ({
  mockImageFromPath: vi.fn(),
  mockWriteImage: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

vi.mock("@tauri-apps/api/image", () => ({
  Image: { fromPath: mockImageFromPath },
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeImage: mockWriteImage,
}));

import {
  flattenPromptForInjection,
  promptWithChatAttachments,
  providerImagePasteKey,
  stageChatImageAttachments,
  submitInputToAgent,
  submitInputToAgents,
} from "./terminalInput";

describe("terminalInput", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockInvoke.mockReset();
    mockImageFromPath.mockReset();
    mockWriteImage.mockReset();
    mockInvoke.mockResolvedValue({
      uuid: "agent-1",
      name: "Coder",
      provider: "codex",
      runtime_state: "live_pty_available",
      delivery_state: "submit_sent_unconfirmed",
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("flattens multiline prompts for injection", () => {
    expect(flattenPromptForInjection("Line one\nLine two\r\nLine three")).toBe(
      "Line one Line two Line three",
    );
  });

  it("returns delivery detail from submit_prompt_to_agent", async () => {
    const result = await submitInputToAgent("agent-1", "hello world");

    expect(result?.delivery_state).toBe("submit_sent_unconfirmed");
    expect(mockInvoke).toHaveBeenNthCalledWith(1, "submit_prompt_to_agent", {
      sessionId: "agent-1",
      prompt: "hello world",
    });
  });

  it("returns all delivery details for multi-agent submission", async () => {
    mockInvoke
      .mockResolvedValueOnce({
        uuid: "agent-1",
        name: "one",
        provider: "codex",
        runtime_state: "live_pty_available",
        delivery_state: "submit_sent_unconfirmed",
      })
      .mockResolvedValueOnce({
        uuid: "agent-2",
        name: "two",
        provider: "claude",
        runtime_state: "live_pty_available",
        delivery_state: "submit_sent_unconfirmed",
      });

    const results = await submitInputToAgents(["agent-1", "agent-2"], "ping");

    expect(results.map((result) => result.uuid)).toEqual(["agent-1", "agent-2"]);
    expect(mockInvoke).toHaveBeenNthCalledWith(1, "submit_prompt_to_agent", {
      sessionId: "agent-1",
      prompt: "ping",
    });
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "submit_prompt_to_agent", {
      sessionId: "agent-2",
      prompt: "ping",
    });
  });

  it("does not use raw terminal input for structured prompt submission", async () => {
    await submitInputToAgent("agent-1", "structured prompt");

    expect(mockInvoke).not.toHaveBeenCalledWith("send_input_to_agent", expect.anything());
    expect(mockInvoke).not.toHaveBeenCalledWith("send_binary_input_to_agent", expect.anything());
  });

  it("uses each provider's image paste shortcut and preserves file paths in the prompt", async () => {
    expect(providerImagePasteKey("claude", "Win32")).toBe("\u001bv");
    expect(providerImagePasteKey("claude", "MacIntel")).toBe("\u0016");
    for (const provider of ["codex", "gemini", "opencode", "antigravity"]) {
      expect(providerImagePasteKey(provider, "Win32")).toBe("\u0016");
    }
    expect(promptWithChatAttachments("", [
      { name: "screen.png", path: "C:\\images\\screen.png" },
      { name: "notes.txt", path: "C:\\notes.txt" },
    ])).toBe("Please inspect the attached files.\n\nAttached files:\n- C:\\images\\screen.png\n- C:\\notes.txt");
  });

  it("writes selected images to the clipboard before invoking the provider paste key", async () => {
    const image = { rid: 1 };
    mockImageFromPath.mockResolvedValue(image);
    mockWriteImage.mockResolvedValue(undefined);
    mockInvoke.mockResolvedValue(undefined);

    await stageChatImageAttachments("agent-1", "claude", [
      { name: "screen.png", path: "C:\\images\\screen.png" },
      { name: "notes.txt", path: "C:\\notes.txt" },
    ], "Win32");

    expect(mockImageFromPath).toHaveBeenCalledWith("C:\\images\\screen.png");
    expect(mockWriteImage).toHaveBeenCalledWith(image);
    expect(mockInvoke).toHaveBeenCalledWith("inject_session_input", {
      sessionId: "agent-1",
      text: "\u001bv",
    });
  });
});

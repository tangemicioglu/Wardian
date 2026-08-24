import { fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { describe, expect, it, vi } from "vitest";
import { AdvancedSettings } from "./AdvancedSettings";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(),
}));

describe("AdvancedSettings", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(writeText).mockReset();
  });

  it("shows the OpenCode agent field for the OpenCode provider", () => {
    const updateField = vi.fn();

    render(
      <AdvancedSettings
        config={{ provider: "opencode" }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));

    const agentInput = screen.getByLabelText("OpenCode Agent");
    fireEvent.change(agentInput, { target: { value: "build" } });

    expect(updateField).toHaveBeenCalledWith("provider_config", {
      type: "opencode",
      agent: "build",
    });
  });

  it("writes Antigravity settings to nested provider config", () => {
    const updateField = vi.fn();

    render(
      <AdvancedSettings
        config={{ provider: "antigravity", provider_config: { type: "antigravity" } }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));
    fireEvent.click(screen.getByLabelText("Sandbox"));
    fireEvent.change(screen.getByLabelText("Agent Mode"), {
      target: { value: "plan" },
    });
    fireEvent.change(screen.getByLabelText("Antigravity Agent"), {
      target: { value: "reviewer" },
    });
    fireEvent.change(screen.getByLabelText("Print Timeout (Headless)"), {
      target: { value: "90s" },
    });

    expect(updateField).toHaveBeenNthCalledWith(1, "provider_config", {
      type: "antigravity",
      sandbox: true,
    });
    expect(updateField).toHaveBeenNthCalledWith(2, "provider_config", {
      type: "antigravity",
      mode: "plan",
    });
    expect(updateField).toHaveBeenNthCalledWith(3, "provider_config", {
      type: "antigravity",
      agent: "reviewer",
    });
    expect(updateField).toHaveBeenNthCalledWith(4, "provider_config", {
      type: "antigravity",
      print_timeout: "90s",
    });
  });

  it("writes Codex sandbox settings to nested provider config", () => {
    const updateField = vi.fn();

    render(
      <AdvancedSettings
        config={{ provider: "codex", provider_config: { type: "codex" } }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));
    fireEvent.change(screen.getByLabelText("Sandbox Mode"), {
      target: { value: "workspace-write" },
    });

    expect(updateField).toHaveBeenCalledWith("provider_config", {
      type: "codex",
      sandbox_mode: "workspace-write",
    });
  });

  it("only renders controls for the selected provider", () => {
    const updateField = vi.fn();

    render(
      <AdvancedSettings
        config={{ provider: "claude", provider_config: { type: "claude" } }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));

    expect(screen.getByLabelText("Permission Mode")).toBeInTheDocument();
    expect(screen.queryByLabelText("Sandbox Mode")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("OpenCode Agent")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Print Timeout (Headless)")).not.toBeInTheDocument();
  });

  it("shows only current interactive provider options", () => {
    const updateField = vi.fn();
    const { rerender } = render(
      <AdvancedSettings
        config={{ provider: "claude", provider_config: { type: "claude" } }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));
    const permissionMode = screen.getByLabelText("Permission Mode");
    expect(permissionMode).toHaveTextContent("Accept edits");
    expect(permissionMode).not.toHaveTextContent("Auto Accept");
    fireEvent.change(permissionMode, { target: { value: "acceptEdits" } });
    expect(updateField).toHaveBeenCalledWith("provider_config", {
      type: "claude",
      permission_mode: "acceptEdits",
    });

    rerender(
      <AdvancedSettings
        config={{ provider: "gemini", provider_config: { type: "gemini" } }}
        updateField={updateField}
      />,
    );
    expect(screen.queryByText("Exp. ACP")).not.toBeInTheDocument();
    expect(screen.queryByText("Output Format")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("YOLO")).not.toBeInTheDocument();
  });

  it("hides unsupported shared controls and exposes OpenCode auto-approval", () => {
    const updateField = vi.fn();
    render(
      <AdvancedSettings
        config={{ provider: "opencode", provider_config: { type: "opencode" } }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));
    expect(screen.queryByText("Include Directories")).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Auto-approve (dangerous)"));
    expect(updateField).toHaveBeenCalledWith("provider_config", {
      type: "opencode",
      auto: true,
    });
  });

  it("labels Pi project trust separately from sandboxing", () => {
    const updateField = vi.fn();
    render(
      <AdvancedSettings
        config={{ provider: "pi", provider_config: { type: "pi" } }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));
    fireEvent.change(screen.getByLabelText("Project Extensions and Skills"), {
      target: { value: "ignore" },
    });
    fireEvent.click(screen.getByLabelText("Offline startup"));

    expect(screen.getByText(/does not sandbox shell tools or extensions/i)).toBeInTheDocument();
    expect(updateField).toHaveBeenNthCalledWith(1, "provider_config", {
      type: "pi",
      project_trust: "ignore",
    });
    expect(updateField).toHaveBeenNthCalledWith(2, "provider_config", {
      type: "pi",
      offline: true,
    });
  });

  it("edits regular session persistence from advanced settings", () => {
    const updateField = vi.fn();

    render(
      <AdvancedSettings
        config={{ provider: "claude", session_persistence: "default" }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));
    fireEvent.change(screen.getByLabelText("Regular Session Resume"), {
      target: { value: "fresh" },
    });

    expect(updateField).toHaveBeenCalledWith("session_persistence", "fresh");
  });

  it("edits the per-agent conversation logging override from advanced settings", () => {
    const updateField = vi.fn();

    render(
      <AdvancedSettings
        config={{ provider: "claude", conversation_logging: "default" }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));
    fireEvent.change(screen.getByLabelText("Conversation Logging"), {
      target: { value: "disabled" },
    });

    expect(updateField).toHaveBeenCalledWith("conversation_logging", "disabled");
  });

  it("places regular session resume outside provider parameters", () => {
    const updateField = vi.fn();
    const { container } = render(
      <AdvancedSettings
        config={{ provider: "claude", session_persistence: "default" }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));

    const text = container.textContent ?? "";
    expect(text.indexOf("Regular Session Resume")).toBeGreaterThanOrEqual(0);
    expect(text.indexOf("Provider Parameters")).toBeGreaterThanOrEqual(0);
    expect(text.indexOf("Regular Session Resume")).toBeLessThan(
      text.indexOf("Provider Parameters"),
    );
  });

  it("copies the full agent command before provider parameters for configured agents", async () => {
    const updateField = vi.fn();
    vi.mocked(invoke).mockResolvedValue("codex resume provider-session");

    const { container } = render(
      <AdvancedSettings
        config={{
          session_id: "agent-1",
          session_name: "CoderOne",
          agent_class: "Coder",
          folder: "C:/repo",
          provider: "codex",
          is_off: true,
          resume_session: "provider-session",
        }}
        showCopyFullCommand
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));

    const text = container.textContent ?? "";
    expect(text.indexOf("Copy Full Agent Command")).toBeGreaterThanOrEqual(0);
    expect(text.indexOf("Provider Parameters")).toBeGreaterThanOrEqual(0);
    expect(text.indexOf("Copy Full Agent Command")).toBeLessThan(
      text.indexOf("Provider Parameters"),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Copy Full Agent Command" }),
    );

    expect(invoke).toHaveBeenCalledWith("build_agent_cli_command", {
      sessionId: "agent-1",
    });
    expect(await screen.findByText("Copied!")).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledWith("codex resume provider-session");
  });

  it("does not show the full command copy control for spawn-only advanced settings", () => {
    const updateField = vi.fn();

    render(
      <AdvancedSettings
        config={{ provider: "claude" }}
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));

    expect(
      screen.queryByRole("button", { name: "Copy Full Agent Command" }),
    ).not.toBeInTheDocument();
  });

  it("does not overwrite the clipboard when full command generation fails", async () => {
    const updateField = vi.fn();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.mocked(invoke).mockRejectedValue(new Error("missing resume session"));

    render(
      <AdvancedSettings
        config={{
          session_id: "agent-1",
          session_name: "CoderOne",
          agent_class: "Coder",
          folder: "C:/repo",
          provider: "codex",
          is_off: true,
        }}
        showCopyFullCommand
        updateField={updateField}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Advanced Settings" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Copy Full Agent Command" }),
    );

    expect(await screen.findByText("Copy failed")).toBeInTheDocument();
    expect(writeText).not.toHaveBeenCalled();
    consoleError.mockRestore();
  });
});

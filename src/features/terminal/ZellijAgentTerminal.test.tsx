import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  ZellijAgentTerminal,
  ZellijAgentTerminalHost,
  useZellijPresentationStore,
} from "./ZellijAgentTerminal";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("./AgentTerminal", () => ({
  AgentTerminal: (props: { sessionId: string; presentationId: string }) => (
    <div
      data-presentation-id={props.presentationId}
      data-session-id={props.sessionId}
      data-testid="live-habitat-terminal"
    />
  ),
}));

function terminal(agentId: string) {
  return (
    <ZellijAgentTerminal
      sessionId={agentId}
      presentationId={`agents:${agentId}`}
      visibility="visible"
      renderState="mounted"
      requestedInteraction="interactive"
      provider="mock"
      theme="dark"
    />
  );
}

function renderTerminals(...terminals: ReactNode[]) {
  return render(
    <>
      <ZellijAgentTerminalHost />
      {terminals}
    </>,
  );
}

describe("ZellijAgentTerminal", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    useZellijPresentationStore.setState({
      activeAgentId: null,
      activeTargetId: null,
      activationSerial: 0,
      slots: new Map(),
    });
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "activate_zellij_agent_terminal") {
        return Promise.resolve("__wardian_habitat_zellij__");
      }
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          habitat_terminal_session_id: "__wardian_habitat_zellij__",
          generation: 1,
          state: "running",
          content: `${args.sessionId} preview`,
        });
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
  });

  it("mounts one stable Habitat renderer while every other agent remains a preview", async () => {
    renderTerminals(terminal("agent-1"), terminal("agent-2"));

    expect(await screen.findByText("agent-1 preview")).toBeInTheDocument();
    expect(await screen.findByText("agent-2 preview")).toBeInTheDocument();
    expect(screen.queryByTestId("live-habitat-terminal")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Activate terminal for agent-1" }));
    await waitFor(() => expect(screen.getAllByTestId("live-habitat-terminal")).toHaveLength(1));
    expect(screen.getByTestId("live-habitat-terminal")).toHaveAttribute(
      "data-session-id",
      "agent-1",
    );
    expect(screen.getByTestId("live-habitat-terminal")).toHaveAttribute(
      "data-presentation-id",
      "desktop:zellij-habitat-terminal",
    );

    fireEvent.click(screen.getByRole("button", { name: "Activate terminal for agent-2" }));
    await waitFor(() => {
      expect(useZellijPresentationStore.getState().activeAgentId).toBe("agent-2");
      expect(screen.getAllByTestId("live-habitat-terminal")).toHaveLength(1);
    });
  });

  it("keeps duplicate surfaces for the active agent to one live renderer", async () => {
    renderTerminals(
      terminal("agent-1"),
      <ZellijAgentTerminal
        key="second-surface"
        sessionId="agent-1"
        presentationId="agent-session:agent-1"
        visibility="visible"
        renderState="mounted"
        requestedInteraction="interactive"
        provider="mock"
        theme="dark"
      />,
    );

    const previews = await screen.findAllByRole("button", {
      name: "Activate terminal for agent-1",
    });
    fireEvent.click(previews[0]);

    await waitFor(() => expect(screen.getAllByTestId("live-habitat-terminal")).toHaveLength(1));
    expect(screen.getAllByRole("button", { name: "Activate terminal for agent-1" })).toHaveLength(1);
  });

  it("keeps unavailable previews noninteractive and exposes recovery state", async () => {
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          habitat_terminal_session_id: "__wardian_habitat_zellij__",
          generation: null,
          state: "unavailable",
          content: "",
        });
      }
      return Promise.reject(new Error("should not activate"));
    });
    renderTerminals(terminal("agent-off"));

    const button = await screen.findByRole("button", {
      name: "Activate terminal for agent-off",
    });
    expect(button).toBeDisabled();
    expect(screen.getByText("Terminal engine unavailable")).toBeInTheDocument();
  });
});

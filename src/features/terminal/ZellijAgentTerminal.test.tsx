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
        return Promise.resolve(args.sessionId);
      }
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          terminal_session_id: args.sessionId,
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

  it("serializes rapid activation so the rendered agent matches the last focused pane", async () => {
    let releaseFirst!: () => void;
    const firstActivation = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command !== "activate_zellij_agent_terminal") {
        return Promise.reject(new Error(`Unexpected command ${command}`));
      }
      return args.sessionId === "agent-1"
        ? firstActivation.then(() => args.sessionId)
        : Promise.resolve(args.sessionId);
    });
    const slots = new Map([
      ["slot-1", { agentId: "agent-1", node: document.createElement("div"), props: {} as never }],
      ["slot-2", { agentId: "agent-2", node: document.createElement("div"), props: {} as never }],
    ]);
    useZellijPresentationStore.setState({ slots });

    const first = useZellijPresentationStore.getState().activate("agent-1", "slot-1");
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    const second = useZellijPresentationStore.getState().activate("agent-2", "slot-2");
    expect(invokeMock).toHaveBeenCalledTimes(1);

    releaseFirst();
    await Promise.all([first, second]);

    expect(invokeMock.mock.calls.filter(([command]) => command === "activate_zellij_agent_terminal"))
      .toEqual([
        ["activate_zellij_agent_terminal", { sessionId: "agent-1" }],
        ["activate_zellij_agent_terminal", { sessionId: "agent-2" }],
      ]);
    expect(useZellijPresentationStore.getState().activeAgentId).toBe("agent-2");
    expect(useZellijPresentationStore.getState().activeTargetId).toBe("slot-2");
  });

  it("reconciles focus when an in-flight activation target is removed", async () => {
    let releaseActivation!: () => void;
    const pendingActivation = new Promise<void>((resolve) => {
      releaseActivation = resolve;
    });
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command !== "activate_zellij_agent_terminal") {
        return Promise.reject(new Error(`Unexpected command ${command}`));
      }
      if (args.sessionId === "agent-2" && invokeMock.mock.calls.length === 1) {
        return pendingActivation.then(() => args.sessionId);
      }
      return Promise.resolve(args.sessionId);
    });
    const slot1 = { agentId: "agent-1", node: document.createElement("div"), props: {} as never };
    const slot2 = { agentId: "agent-2", node: document.createElement("div"), props: {} as never };
    useZellijPresentationStore.setState({
      activeAgentId: "agent-1",
      activeTargetId: "slot-1",
      slots: new Map([["slot-1", slot1], ["slot-2", slot2]]),
    });

    const staleActivation = useZellijPresentationStore.getState().activate("agent-2", "slot-2");
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    useZellijPresentationStore.getState().removeSlot("slot-2");
    releaseActivation();
    await staleActivation;
    await waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));

    expect(invokeMock.mock.calls.slice(0, 2)).toEqual([
      ["activate_zellij_agent_terminal", { sessionId: "agent-2" }],
      ["activate_zellij_agent_terminal", { sessionId: "agent-1" }],
    ]);
    expect(useZellijPresentationStore.getState().activeAgentId).toBe("agent-1");
    expect(useZellijPresentationStore.getState().activeTargetId).toBe("slot-1");

    useZellijPresentationStore.getState().upsertSlot("slot-2", slot2);
    expect(useZellijPresentationStore.getState().activeTargetId).toBe("slot-1");
    await useZellijPresentationStore.getState().activate("agent-2", "slot-2");
    expect(useZellijPresentationStore.getState().activeAgentId).toBe("agent-2");
    expect(useZellijPresentationStore.getState().activeTargetId).toBe("slot-2");
    expect(invokeMock).toHaveBeenLastCalledWith(
      "activate_zellij_agent_terminal",
      { sessionId: "agent-2" },
    );
  });

  it("keeps unavailable previews noninteractive and exposes recovery state", async () => {
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          terminal_session_id: args.sessionId,
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

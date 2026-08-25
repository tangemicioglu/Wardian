import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConfig } from "../../types";
import { AgentSessionSurface } from "../workbench/surfaces/AgentSessionSurface";
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
  AgentTerminal: (props: {
    sessionId: string;
    presentationId: string;
    visibility: "visible" | "hidden";
    renderState: "mounted" | "suspended";
    requestedInteraction: "interactive" | "read_only";
  }) => (
    <div
      data-presentation-id={props.presentationId}
      data-render-state={props.renderState}
      data-requested-interaction={props.requestedInteraction}
      data-session-id={props.sessionId}
      data-testid="live-habitat-terminal"
    >
      <div
        data-terminal-session-id={props.sessionId}
        data-testid="agent-terminal-host"
        style={{ visibility: props.visibility }}
      >
        <textarea aria-label="Terminal input" className="xterm-helper-textarea" />
      </div>
    </div>
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
      brokerOwners: new Map(),
      focusRequestSerial: 0,
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
          broker_generation: 1,
          broker_lease_epoch: 1,
          broker_owner_presentation_id: null,
          broker_activation_pending: false,
          state: "running",
          content: `${args.sessionId} preview`,
        });
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
  });

  it("keeps one stable Habitat renderer and focuses previews without an activation mode", async () => {
    renderTerminals(terminal("agent-1"), terminal("agent-2"));

    expect(await screen.findByText("agent-1 preview")).toBeInTheDocument();
    expect(await screen.findByText("agent-2 preview")).toBeInTheDocument();
    expect(screen.queryByTestId("live-habitat-terminal")).not.toBeInTheDocument();
    expect(invokeMock.mock.calls.filter(
      ([command]) => command === "activate_zellij_agent_terminal",
    )).toHaveLength(0);
    expect(useZellijPresentationStore.getState().activeTargetId).toBeNull();

    expect(screen.queryByText("Activate terminal")).not.toBeInTheDocument();
    fireEvent.pointerDown(screen.getByRole("application", { name: "Terminal for agent-1" }));
    await waitFor(() => expect(screen.getAllByTestId("live-habitat-terminal")).toHaveLength(1));
    screen.getByTestId("live-habitat-terminal").setAttribute("data-stable-renderer", "true");
    expect(screen.getByTestId("live-habitat-terminal")).toHaveAttribute(
      "data-session-id",
      "agent-1",
    );
    expect(screen.getByTestId("live-habitat-terminal")).toHaveAttribute(
      "data-presentation-id",
      "desktop:zellij-habitat-terminal",
    );

    fireEvent.pointerDown(screen.getByRole("application", { name: "Terminal for agent-2" }));
    await waitFor(() => {
      expect(useZellijPresentationStore.getState().activeAgentId).toBe("agent-2");
      expect(screen.getAllByTestId("live-habitat-terminal")).toHaveLength(1);
      expect(screen.getByTestId("live-habitat-terminal")).toHaveAttribute(
        "data-stable-renderer",
        "true",
      );
      expect(screen.getByLabelText("Terminal input")).toHaveFocus();
    });
  });

  it("repositions the singleton after a selected slot moves without resizing", async () => {
    let slotLeft = 24;
    const boundsSpy = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function getBoundingClientRect(this: HTMLElement) {
        const isTerminalSlot = this.hasAttribute("data-zellij-presentation");
        const left = isTerminalSlot ? slotLeft : 0;
        const top = isTerminalSlot ? 40 : 0;
        const width = isTerminalSlot ? 320 : 1024;
        const height = isTerminalSlot ? 240 : 768;
        return {
          x: left,
          y: top,
          left,
          top,
          right: left + width,
          bottom: top + height,
          width,
          height,
          toJSON: () => ({}),
        } as DOMRect;
      });
    try {
      renderTerminals(terminal("agent-1"));
      fireEvent.pointerDown(await screen.findByRole("application", {
        name: "Terminal for agent-1",
      }));
      const viewport = await waitFor(() => {
        const next = document.querySelector<HTMLElement>(
          '[data-zellij-singleton-viewport="true"]',
        );
        expect(next).toHaveStyle({ left: "24px", top: "40px" });
        return next!;
      });

      slotLeft = 264;
      document.querySelector<HTMLElement>('[data-zellij-presentation="live"]')
        ?.parentElement?.setAttribute("data-layout-revision", "2");

      await waitFor(() => expect(viewport).toHaveStyle({ left: "264px", top: "40px" }));
    } finally {
      boundsSpy.mockRestore();
    }
  });

  it("keeps the renderer mounted but releases interaction when the final card closes", async () => {
    const view = render(
      <>
        <ZellijAgentTerminalHost />
        {terminal("agent-1")}
      </>,
    );
    fireEvent.pointerDown(await screen.findByRole("application", { name: "Terminal for agent-1" }));
    await waitFor(() => expect(screen.getByTestId("live-habitat-terminal")).toBeInTheDocument());
    const renderer = screen.getByTestId("live-habitat-terminal");

    view.rerender(<ZellijAgentTerminalHost />);

    await waitFor(() => {
      expect(screen.getByTestId("live-habitat-terminal")).toBe(renderer);
      expect(renderer).toHaveAttribute("data-requested-interaction", "read_only");
      expect(screen.getByTestId("agent-terminal-host")).toHaveStyle({ visibility: "hidden" });
    });
  });

  it("keeps read-only previews from claiming the singleton broker owner", async () => {
    renderTerminals(
      <ZellijAgentTerminal
        sessionId="agent-mirror"
        presentationId="agent-session:mirror"
        visibility="visible"
        renderState="mounted"
        requestedInteraction="read_only"
        provider="mock"
        theme="dark"
      />,
    );

    const preview = await screen.findByRole("application", {
      name: "Terminal for agent-mirror",
    });
    await waitFor(() => expect(preview).toHaveAttribute("aria-disabled", "true"));
    expect(preview).toHaveAttribute("tabindex", "-1");
    fireEvent.pointerDown(preview);

    expect(invokeMock).not.toHaveBeenCalledWith(
      "activate_zellij_agent_terminal",
      expect.anything(),
    );
    expect(screen.queryByTestId("live-habitat-terminal")).not.toBeInTheDocument();
  });

  it("makes only the remotely owned agent read-only in the card grid", async () => {
    renderTerminals(terminal("agent-1"), terminal("agent-2"));
    fireEvent.pointerDown(await screen.findByRole("application", { name: "Terminal for agent-1" }));
    const renderer = await screen.findByTestId("live-habitat-terminal");
    await waitFor(() => expect(useZellijPresentationStore.getState().activeAgentId).toBe("agent-1"));
    const activationCount = invokeMock.mock.calls.filter(
      ([command]) => command === "activate_zellij_agent_terminal",
    ).length;

    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "activate_zellij_agent_terminal") return Promise.resolve(args.sessionId);
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          terminal_session_id: args.sessionId,
          generation: 1,
          broker_generation: 1,
          broker_lease_epoch: 2,
          broker_owner_presentation_id: args.sessionId === "agent-1"
            ? "remote:paired-device"
            : null,
          broker_activation_pending: false,
          state: "running",
          content: `${args.sessionId} preview`,
        });
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });

    act(() => {
      useZellijPresentationStore.getState().setBrokerOwner(
        "agent-1",
        1,
        2,
        "remote:paired-device",
      );
    });

    const preview = await screen.findByRole("application", { name: "Terminal for agent-1" });
    await waitFor(() => {
      expect(renderer).toHaveAttribute("data-requested-interaction", "read_only");
      expect(preview).toHaveAttribute("aria-disabled", "true");
      expect(document.querySelector('[data-zellij-presentation="renderer"]')).toHaveStyle({
        pointerEvents: "none",
        visibility: "hidden",
      });
    });
    fireEvent.pointerDown(preview);
    expect(invokeMock.mock.calls.filter(
      ([command]) => command === "activate_zellij_agent_terminal",
    )).toHaveLength(activationCount);

    const otherPreview = screen.getByRole("application", { name: "Terminal for agent-2" });
    expect(otherPreview).toHaveAttribute("aria-disabled", "false");
    fireEvent.pointerDown(otherPreview);
    await waitFor(() => {
      expect(useZellijPresentationStore.getState().activeAgentId).toBe("agent-2");
      expect(invokeMock.mock.calls.filter(
        ([command]) => command === "activate_zellij_agent_terminal",
      )).toHaveLength(activationCount + 1);
    });
  });

  it("hides the singleton and disables its card while ownership is transferring", async () => {
    renderTerminals(terminal("agent-1"));
    const preview = await screen.findByRole("application", { name: "Terminal for agent-1" });
    fireEvent.pointerDown(preview);
    const renderer = await screen.findByTestId("live-habitat-terminal");

    act(() => {
      useZellijPresentationStore.getState().setBrokerOwner(
        "agent-1",
        1,
        2,
        null,
        true,
        "live",
      );
    });

    const transferPreview = await screen.findByRole("application", {
      name: "Terminal for agent-1",
    });
    expect(transferPreview).toHaveAttribute("aria-disabled", "true");
    expect(renderer).toHaveAttribute("data-requested-interaction", "read_only");
    expect(document.querySelector('[data-zellij-presentation="renderer"]')).toHaveStyle({
      pointerEvents: "none",
      visibility: "hidden",
    });
  });

  it("retains remote ownership for an inactive card across other-agent selections", async () => {
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "activate_zellij_agent_terminal") return Promise.resolve(args.sessionId);
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          terminal_session_id: args.sessionId,
          generation: 1,
          broker_generation: 1,
          broker_lease_epoch: 2,
          broker_owner_presentation_id: args.sessionId === "agent-2"
            ? "remote:paired-device"
            : null,
          broker_activation_pending: false,
          state: "running",
          content: `${args.sessionId} preview`,
        });
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    renderTerminals(terminal("agent-1"), terminal("agent-2"));

    const first = await screen.findByRole("application", { name: "Terminal for agent-1" });
    const remotelyOwned = await screen.findByRole("application", {
      name: "Terminal for agent-2",
    });
    await waitFor(() => expect(remotelyOwned).toHaveAttribute("aria-disabled", "true"));

    fireEvent.pointerDown(first);
    await waitFor(() => expect(useZellijPresentationStore.getState().activeAgentId).toBe("agent-1"));
    const activationCount = invokeMock.mock.calls.filter(
      ([command]) => command === "activate_zellij_agent_terminal",
    ).length;

    fireEvent.pointerDown(remotelyOwned);
    expect(invokeMock.mock.calls.filter(
      ([command]) => command === "activate_zellij_agent_terminal",
    )).toHaveLength(activationCount);
    expect(useZellijPresentationStore.getState().brokerOwners.get("agent-2")).toEqual({
      generation: 1,
      leaseEpoch: 2,
      owner: "remote:paired-device",
      activationPending: false,
      source: "preview",
    });
  });

  it("updates an inactive Agent Session surface from shared desktop and remote ownership", async () => {
    const agent: AgentConfig = {
      session_id: "agent-1",
      session_name: "Mendel",
      agent_class: "Coder",
      folder: "/workspace/wardian",
      provider: "mock",
      is_off: false,
    };
    render(
      <>
        <ZellijAgentTerminalHost />
        {terminal("agent-1")}
        <AgentSessionSurface
          surface_id="agent-session-1"
          resource_key="agent-1"
          agent={agent}
          theme="dark"
        />
      </>,
    );

    const previews = await screen.findAllByRole("application", {
      name: "Terminal for agent-1",
    });
    fireEvent.pointerDown(previews[0]);
    await waitFor(() => expect(useZellijPresentationStore.getState().activeTargetId)
      .toBe("agents:agent-1:agent-1"));

    act(() => {
      useZellijPresentationStore.getState().setBrokerOwner(
        "agent-1",
        1,
        2,
        "desktop:zellij-habitat-terminal",
      );
    });
    expect(screen.getByTestId("agent-session-presentation-mode")).toHaveTextContent("Mirror");
    expect(screen.queryByTestId("agent-session-read-only")).not.toBeInTheDocument();

    act(() => {
      useZellijPresentationStore.getState().setBrokerOwner(
        "agent-1",
        1,
        3,
        "remote:paired-device",
      );
    });
    expect(screen.getByTestId("agent-session-presentation-mode")).toHaveTextContent("Mirror");
    expect(screen.getByTestId("agent-session-read-only")).toHaveTextContent("Read only");
  });

  it("clears a stale remote owner when the broker advances to a replacement generation", async () => {
    let previewGeneration = 1;
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "activate_zellij_agent_terminal") return Promise.resolve(args.sessionId);
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          terminal_session_id: args.sessionId,
          generation: previewGeneration,
          broker_generation: previewGeneration,
          broker_lease_epoch: previewGeneration,
          broker_owner_presentation_id: null,
          broker_activation_pending: false,
          state: "running",
          content: `${args.sessionId} generation ${previewGeneration}`,
        });
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    renderTerminals(terminal("agent-1"));
    fireEvent.pointerDown(await screen.findByRole("application", { name: "Terminal for agent-1" }));
    const renderer = await screen.findByTestId("live-habitat-terminal");

    useZellijPresentationStore.getState().setBrokerOwner(
      "agent-1",
      1,
      1,
      "remote:old-generation",
    );
    previewGeneration = 2;

    await waitFor(() => {
      expect(useZellijPresentationStore.getState().brokerOwners.get("agent-1")).toEqual({
        generation: 2,
        leaseEpoch: 2,
        owner: null,
        activationPending: false,
        source: "preview",
      });
      expect(renderer).toHaveAttribute("data-requested-interaction", "interactive");
    }, { timeout: 3000 });
  });

  it("clears a known remote owner after an authoritative no-broker preview", async () => {
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          terminal_session_id: args.sessionId,
          generation: null,
          broker_generation: null,
          broker_lease_epoch: null,
          broker_owner_presentation_id: null,
          broker_activation_pending: false,
          state: "unavailable",
          content: "",
        });
      }
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    useZellijPresentationStore.getState().setBrokerOwner(
      "agent-1",
      4,
      8,
      "remote:paired-device",
    );

    renderTerminals(terminal("agent-1"));

    await screen.findByText("Terminal engine unavailable");
    expect(useZellijPresentationStore.getState().brokerOwners.get("agent-1")).toEqual({
      generation: null,
      leaseEpoch: null,
      owner: null,
      activationPending: false,
      source: "preview",
    });
  });

  it("keeps a known remote owner when the preview request itself fails", async () => {
    invokeMock.mockRejectedValue(new Error("transport unavailable"));
    useZellijPresentationStore.getState().setBrokerOwner(
      "agent-1",
      4,
      8,
      "remote:paired-device",
    );

    renderTerminals(terminal("agent-1"));

    await screen.findByText("Terminal engine unavailable");
    expect(useZellijPresentationStore.getState().brokerOwners.get("agent-1")).toEqual({
      generation: 4,
      leaseEpoch: 8,
      owner: "remote:paired-device",
      activationPending: false,
      source: "live",
    });
  });

  it("orders preview ownership across two slots for the same agent", async () => {
    let resolveFirstPreview: ((preview: Record<string, unknown>) => void) | undefined;
    let previewCalls = 0;
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command !== "get_zellij_terminal_preview") {
        return Promise.reject(new Error(`Unexpected command ${command}`));
      }
      previewCalls += 1;
      if (previewCalls === 1) {
        return new Promise((resolve) => { resolveFirstPreview = resolve; });
      }
      return Promise.resolve({
        session_id: args.sessionId,
        terminal_session_id: args.sessionId,
        generation: 5,
        broker_generation: 5,
        broker_lease_epoch: 9,
        broker_owner_presentation_id: "remote:new-generation",
        broker_activation_pending: false,
        state: "running",
        content: "new generation",
      });
    });

    renderTerminals(
      terminal("agent-1"),
      <ZellijAgentTerminal
        sessionId="agent-1"
        presentationId="agent-session:agent-1"
        visibility="visible"
        renderState="mounted"
        requestedInteraction="interactive"
        provider="mock"
        theme="dark"
      />,
    );
    await waitFor(() => expect(useZellijPresentationStore.getState().brokerOwners.get("agent-1"))
      .toEqual({
        generation: 5,
        leaseEpoch: 9,
        owner: "remote:new-generation",
        activationPending: false,
        source: "preview",
      }), { timeout: 2500 });

    await act(async () => {
      resolveFirstPreview?.({
        session_id: "agent-1",
        terminal_session_id: "agent-1",
        generation: null,
        broker_generation: null,
        broker_lease_epoch: null,
        broker_owner_presentation_id: null,
        broker_activation_pending: false,
        state: "unavailable",
        content: "",
      });
      await Promise.resolve();
    });

    expect(useZellijPresentationStore.getState().brokerOwners.get("agent-1")).toEqual({
      generation: 5,
      leaseEpoch: 9,
      owner: "remote:new-generation",
      activationPending: false,
      source: "preview",
    });
  });

  it("does not let a delayed preview overwrite a live owner at the same lease", async () => {
    let resolveLatestPreview: ((preview: Record<string, unknown>) => void) | undefined;
    let previewCalls = 0;
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command !== "get_zellij_terminal_preview") {
        return Promise.reject(new Error(`Unexpected command ${command}`));
      }
      previewCalls += 1;
      if (previewCalls === 2) {
        return new Promise((resolve) => { resolveLatestPreview = resolve; });
      }
      return Promise.resolve({
        session_id: args.sessionId,
        terminal_session_id: args.sessionId,
        generation: 5,
        broker_generation: 5,
        broker_lease_epoch: 9,
        broker_owner_presentation_id: null,
        broker_activation_pending: false,
        state: "running",
        content: "first slot",
      });
    });

    renderTerminals(
      terminal("agent-1"),
      <ZellijAgentTerminal
        sessionId="agent-1"
        presentationId="agent-session:agent-1"
        visibility="visible"
        renderState="mounted"
        requestedInteraction="interactive"
        provider="mock"
        theme="dark"
      />,
    );
    await waitFor(() => expect(previewCalls).toBe(2));
    act(() => {
      useZellijPresentationStore.getState().setBrokerOwner(
        "agent-1",
        5,
        9,
        "remote:committed-owner",
        false,
        "live",
      );
    });

    await act(async () => {
      resolveLatestPreview?.({
        session_id: "agent-1",
        terminal_session_id: "agent-1",
        generation: 5,
        broker_generation: 5,
        broker_lease_epoch: 9,
        broker_owner_presentation_id: null,
        broker_activation_pending: true,
        state: "running",
        content: "stale pending frame",
      });
      await Promise.resolve();
    });

    expect(useZellijPresentationStore.getState().brokerOwners.get("agent-1")).toEqual({
      generation: 5,
      leaseEpoch: 9,
      owner: "remote:committed-owner",
      activationPending: false,
      source: "live",
    });
  });

  it("allows a current preview to resolve a paused live transfer", () => {
    useZellijPresentationStore.getState().setBrokerOwner(
      "agent-1",
      5,
      9,
      null,
      true,
      "live",
    );
    useZellijPresentationStore.getState().setBrokerOwner(
      "agent-1",
      5,
      9,
      "remote:committed-owner",
      false,
      "preview",
    );

    expect(useZellijPresentationStore.getState().brokerOwners.get("agent-1")).toEqual({
      generation: 5,
      leaseEpoch: 9,
      owner: "remote:committed-owner",
      activationPending: false,
      source: "preview",
    });
  });

  it("releases a card latch when its queued activation is skipped", async () => {
    let releaseFirstActivation: (() => void) | undefined;
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          terminal_session_id: args.sessionId,
          generation: 1,
          broker_generation: 1,
          broker_lease_epoch: 1,
          broker_owner_presentation_id: null,
          broker_activation_pending: false,
          state: "running",
          content: `${args.sessionId} preview`,
        });
      }
      if (command === "activate_zellij_agent_terminal" && args.sessionId === "agent-1") {
        return new Promise((resolve) => {
          releaseFirstActivation = () => resolve(args.sessionId);
        });
      }
      if (command === "activate_zellij_agent_terminal") return Promise.resolve(args.sessionId);
      return Promise.reject(new Error(`Unexpected command ${command}`));
    });
    renderTerminals(terminal("agent-1"), terminal("agent-2"));
    const first = await screen.findByRole("application", { name: "Terminal for agent-1" });
    const second = await screen.findByRole("application", { name: "Terminal for agent-2" });

    fireEvent.pointerDown(first);
    await waitFor(() => expect(releaseFirstActivation).toBeDefined());
    fireEvent.pointerDown(second);
    act(() => {
      useZellijPresentationStore.getState().setBrokerOwner(
        "agent-2",
        1,
        2,
        "remote:paired-device",
        false,
        "live",
      );
      releaseFirstActivation?.();
    });
    await waitFor(() => expect(invokeMock.mock.calls.filter(
      ([command]) => command === "activate_zellij_agent_terminal",
    )).toHaveLength(1));

    act(() => {
      useZellijPresentationStore.getState().setBrokerOwner(
        "agent-2",
        1,
        3,
        null,
        false,
        "live",
      );
    });
    fireEvent.pointerDown(screen.getByRole("application", { name: "Terminal for agent-2" }));

    await waitFor(() => {
      expect(useZellijPresentationStore.getState().activeAgentId).toBe("agent-2");
      expect(invokeMock.mock.calls.filter(
        ([command]) => command === "activate_zellij_agent_terminal",
      )).toHaveLength(2);
    });
  });

  it("releases an active owner when its slot becomes read-only or suspended", async () => {
    const view = render(
      <>
        <ZellijAgentTerminalHost />
        {terminal("agent-1")}
      </>,
    );
    fireEvent.pointerDown(await screen.findByRole("application", { name: "Terminal for agent-1" }));
    const renderer = await screen.findByTestId("live-habitat-terminal");

    view.rerender(
      <>
        <ZellijAgentTerminalHost />
        <ZellijAgentTerminal
          sessionId="agent-1"
          presentationId="agents:agent-1"
          visibility="visible"
          renderState="mounted"
          requestedInteraction="read_only"
          provider="mock"
          theme="dark"
        />
      </>,
    );

    await waitFor(() => {
      expect(screen.getByTestId("live-habitat-terminal")).toBe(renderer);
      expect(renderer).toHaveAttribute("data-requested-interaction", "read_only");
      expect(screen.getByTestId("agent-terminal-host")).toHaveStyle({ visibility: "hidden" });
      expect(document.querySelector('[data-zellij-presentation="renderer"]')).toHaveStyle({
        pointerEvents: "none",
        visibility: "hidden",
      });
      expect(screen.getByRole("application", { name: "Terminal for agent-1" }))
        .toHaveAttribute("aria-disabled", "true");
    });

    view.rerender(
      <>
        <ZellijAgentTerminalHost />
        <ZellijAgentTerminal
          sessionId="agent-1"
          presentationId="agents:agent-1"
          visibility="hidden"
          renderState="suspended"
          requestedInteraction="read_only"
          provider="mock"
          theme="dark"
        />
      </>,
    );

    await waitFor(() => {
      expect(renderer).toHaveAttribute("data-render-state", "mounted");
      expect(screen.getByTestId("agent-terminal-host")).toHaveStyle({ visibility: "hidden" });
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

    const previews = await screen.findAllByRole("application", {
      name: "Terminal for agent-1",
    });
    await waitFor(() => {
      expect(previews).toHaveLength(2);
      expect(previews[0]).toHaveAttribute("aria-disabled", "false");
    });
    fireEvent.pointerDown(previews[0]);

    await waitFor(() => expect(screen.getAllByTestId("live-habitat-terminal")).toHaveLength(1));
    expect(screen.getAllByRole("application", { name: "Terminal for agent-1" })).toHaveLength(1);
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
    const ownerProps = {
      visibility: "visible",
      renderState: "mounted",
      requestedInteraction: "interactive",
    } as never;
    const slots = new Map([
      ["slot-1", { agentId: "agent-1", node: document.createElement("div"), presentationId: "slot-1", props: ownerProps }],
      ["slot-2", { agentId: "agent-2", node: document.createElement("div"), presentationId: "slot-2", props: ownerProps }],
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
        ["activate_zellij_agent_terminal", { sessionId: "agent-1", brokerGeneration: null }],
        ["activate_zellij_agent_terminal", { sessionId: "agent-2", brokerGeneration: null }],
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
    const ownerProps = {
      visibility: "visible",
      renderState: "mounted",
      requestedInteraction: "interactive",
    } as never;
    const slot1 = { agentId: "agent-1", node: document.createElement("div"), presentationId: "slot-1", props: ownerProps };
    const slot2 = { agentId: "agent-2", node: document.createElement("div"), presentationId: "slot-2", props: ownerProps };
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
      ["activate_zellij_agent_terminal", { sessionId: "agent-2", brokerGeneration: null }],
      ["activate_zellij_agent_terminal", { sessionId: "agent-1", brokerGeneration: null }],
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
      { sessionId: "agent-2", brokerGeneration: null },
    );
  });

  it("keeps unavailable previews noninteractive and exposes recovery state", async () => {
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          terminal_session_id: args.sessionId,
          generation: null,
          broker_generation: null,
          broker_lease_epoch: null,
          broker_owner_presentation_id: null,
          broker_activation_pending: false,
          state: "unavailable",
          content: "",
        });
      }
      return Promise.reject(new Error("should not activate"));
    });
    renderTerminals(terminal("agent-off"));

    const preview = await screen.findByRole("application", {
      name: "Terminal for agent-off",
    });
    expect(preview).toHaveAttribute("aria-disabled", "true");
    expect(preview).toHaveAttribute("tabindex", "-1");
    expect(screen.getByText("Terminal engine unavailable")).toBeInTheDocument();
  });

  it("keeps starting previews noninteractive until the pane is running", async () => {
    invokeMock.mockImplementation((command: string, args: { sessionId: string }) => {
      if (command === "get_zellij_terminal_preview") {
        return Promise.resolve({
          session_id: args.sessionId,
          terminal_session_id: args.sessionId,
          generation: 1,
          broker_generation: 1,
          broker_lease_epoch: 1,
          broker_owner_presentation_id: null,
          broker_activation_pending: false,
          state: "starting",
          content: "",
        });
      }
      return Promise.reject(new Error("should not activate"));
    });
    renderTerminals(terminal("agent-starting"));

    const preview = await screen.findByRole("application", {
      name: "Terminal for agent-starting",
    });
    expect(preview).toHaveAttribute("aria-disabled", "true");
    expect(preview).toHaveAttribute("tabindex", "-1");
    expect(screen.getByText("Starting terminal…")).toBeInTheDocument();
    fireEvent.pointerDown(preview);
    expect(invokeMock).not.toHaveBeenCalledWith("activate_zellij_agent_terminal", expect.anything());
  });
});

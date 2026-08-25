import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { create } from "zustand";
import type {
  TerminalBrokerState,
  TerminalPresentationState,
  TerminalRenderState,
  TerminalRequestedInteraction,
  TerminalVisibility,
} from "../../types";
import { AgentTerminal } from "./AgentTerminal";

const HABITAT_TERMINAL_SESSION_ID = "__wardian_habitat_zellij__";
const HABITAT_TERMINAL_PRESENTATION_ID = "desktop:zellij-habitat-terminal";

type ZellijTerminalPreview = {
  session_id: string;
  habitat_terminal_session_id: string;
  generation: number | null;
  state: "starting" | "running" | "exited" | "unavailable";
  content: string;
};

type LiveTerminalProps = Omit<ZellijAgentTerminalProps, "presentationId">;

type ZellijTerminalSlot = {
  agentId: string;
  node: HTMLElement;
  props: LiveTerminalProps;
};

type ZellijPresentationStore = {
  activeAgentId: string | null;
  activeTargetId: string | null;
  activationSerial: number;
  slots: Map<string, ZellijTerminalSlot>;
  upsertSlot: (targetId: string, slot: ZellijTerminalSlot) => void;
  removeSlot: (targetId: string) => void;
  activate: (agentId: string, targetId: string) => Promise<void>;
};

export const useZellijPresentationStore = create<ZellijPresentationStore>((set, get) => {
  let activationQueue = Promise.resolve();
  return {
    activeAgentId: null,
    activeTargetId: null,
    activationSerial: 0,
    slots: new Map(),
    upsertSlot: (targetId, slot) => {
      const slots = new Map(get().slots);
      slots.set(targetId, slot);
      const adoptTarget = get().activeAgentId === slot.agentId && get().activeTargetId === null;
      set({ slots, ...(adoptTarget ? { activeTargetId: targetId } : {}) });
    },
    removeSlot: (targetId) => {
      const state = get();
      const removed = state.slots.get(targetId);
      const slots = new Map(state.slots);
      slots.delete(targetId);
      if (state.activeTargetId !== targetId) {
        set({ slots });
        return;
      }
      const fallback = removed
        ? Array.from(slots.entries()).find(([, slot]) => slot.agentId === removed.agentId)?.[0]
        : undefined;
      set({ slots, activeTargetId: fallback ?? null });
    },
    activate: (agentId, targetId) => {
      const serial = get().activationSerial + 1;
      set({ activationSerial: serial });
      const activation = activationQueue.then(async () => {
        if (get().activationSerial !== serial) return;
        await invoke<string>("activate_zellij_agent_terminal", { sessionId: agentId });
        if (get().activationSerial === serial && get().slots.has(targetId)) {
          set({ activeAgentId: agentId, activeTargetId: targetId });
        }
      });
      activationQueue = activation.catch(() => undefined);
      return activation;
    },
  };
});

export interface ZellijAgentTerminalProps {
  sessionId: string;
  presentationId: string;
  visibility: TerminalVisibility;
  renderState: TerminalRenderState;
  requestedInteraction: TerminalRequestedInteraction;
  provider?: string;
  isMaximized?: boolean;
  theme: "dark" | "light" | "system";
  workspacePath?: string | null;
  onTitleChange?: (title: string) => void;
  onTerminalFocus?: () => void;
  onPresentationStateChange?: (
    brokerState: TerminalBrokerState,
    presentationState: TerminalPresentationState | null,
  ) => void;
  autoActivateWhenUnowned?: boolean;
  autoFocus?: boolean;
}

/** Mount once at app level. It is the only agent-facing xterm in the process. */
export function ZellijAgentTerminalHost() {
  const target = useZellijPresentationStore((state) => {
    if (!state.activeTargetId) return null;
    return state.slots.get(state.activeTargetId) ?? null;
  });
  if (!target) return null;

  const props = target.props;
  return createPortal(
    <AgentTerminal
      sessionId={target.agentId}
      presentationId={HABITAT_TERMINAL_PRESENTATION_ID}
      visibility={props.visibility}
      renderState={props.renderState}
      requestedInteraction={props.requestedInteraction}
      provider={props.provider}
      isMaximized={props.isMaximized}
      theme={props.theme}
      workspacePath={props.workspacePath}
      onTitleChange={props.onTitleChange}
      onTerminalFocus={props.onTerminalFocus}
      onPresentationStateChange={props.onPresentationStateChange}
      autoActivateWhenUnowned
      autoFocus={props.autoFocus}
    />,
    target.node,
  );
}

/** A terminal slot. Only the selected slot receives the singleton host. */
export function ZellijAgentTerminal({ presentationId, ...props }: ZellijAgentTerminalProps) {
  const { sessionId, visibility, renderState, onTerminalFocus } = props;
  const targetId = `${presentationId}:${sessionId}`;
  const hostRef = useRef<HTMLElement | null>(null);
  const activeTargetId = useZellijPresentationStore((state) => state.activeTargetId);
  const activate = useZellijPresentationStore((state) => state.activate);
  const upsertSlot = useZellijPresentationStore((state) => state.upsertSlot);
  const removeSlot = useZellijPresentationStore((state) => state.removeSlot);
  const [preview, setPreview] = useState<ZellijTerminalPreview | null>(null);
  const [activationError, setActivationError] = useState<string | null>(null);
  const autoActivationAttempted = useRef(false);
  const isLiveTarget = activeTargetId === targetId;

  useEffect(() => {
    const node = hostRef.current;
    if (node) upsertSlot(targetId, { agentId: sessionId, node, props });
  });

  useEffect(() => () => removeSlot(targetId), [removeSlot, targetId]);

  useEffect(() => {
    if (isLiveTarget || visibility !== "visible" || renderState !== "mounted") return;
    let cancelled = false;
    const refresh = async () => {
      try {
        const next = await invoke<ZellijTerminalPreview>("get_zellij_terminal_preview", {
          sessionId,
        });
        if (!cancelled) setPreview(next);
      } catch {
        if (!cancelled) {
          setPreview({
            session_id: sessionId,
            habitat_terminal_session_id: HABITAT_TERMINAL_SESSION_ID,
            generation: null,
            state: "unavailable",
            content: "",
          });
        }
      }
    };
    void refresh();
    const interval = window.setInterval(() => void refresh(), 750);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [isLiveTarget, renderState, sessionId, visibility]);

  useEffect(() => {
    if (
      autoActivationAttempted.current
      || isLiveTarget
      || visibility !== "visible"
      || renderState !== "mounted"
      || (!props.autoFocus && !props.autoActivateWhenUnowned)
    ) return;
    autoActivationAttempted.current = true;
    void activate(sessionId, targetId).catch((error) => {
      setActivationError(error instanceof Error ? error.message : "Terminal activation failed");
    });
  }, [activate, isLiveTarget, props.autoActivateWhenUnowned, props.autoFocus, renderState, sessionId, targetId, visibility]);

  if (isLiveTarget) {
    return (
      <div
        ref={(node) => { hostRef.current = node; }}
        className="h-full min-h-0 w-full min-w-0 overflow-hidden"
        data-zellij-agent-id={sessionId}
        data-zellij-presentation="live"
      />
    );
  }

  const stateCopy = activationError
    ?? (preview?.state === "exited"
      ? "Agent terminal exited"
      : preview?.state === "unavailable"
        ? "Terminal engine unavailable"
        : preview?.state === "running"
          ? "Activate terminal"
          : "Starting terminal…");

  return (
    <button
      ref={(node) => { hostRef.current = node; }}
      aria-label={`Activate terminal for ${sessionId}`}
      className="group relative flex h-full w-full min-h-0 min-w-0 cursor-pointer flex-col overflow-hidden bg-[var(--color-wardian-bg)] text-left"
      data-zellij-agent-id={sessionId}
      data-zellij-presentation="preview"
      disabled={preview?.state !== "running"}
      onClick={() => {
        setActivationError(null);
        onTerminalFocus?.();
        void activate(sessionId, targetId).catch((error) => {
          setActivationError(error instanceof Error ? error.message : "Terminal activation failed");
        });
      }}
      type="button"
    >
      <pre aria-hidden="true" className="min-h-0 flex-1 overflow-hidden whitespace-pre-wrap p-2 font-mono text-[11px] leading-4 text-[var(--color-wardian-text-muted)]">
        {preview?.content || ""}
      </pre>
      <span className="absolute inset-x-0 bottom-0 border-t border-wardian-border bg-[var(--color-wardian-sidebar-secondary)] px-2 py-1 text-center text-[11px] font-medium text-primary">
        {stateCopy}
      </span>
    </button>
  );
}

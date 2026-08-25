import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
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

export const HABITAT_TERMINAL_PRESENTATION_ID = "desktop:zellij-habitat-terminal";

type ZellijTerminalPreview = {
  session_id: string;
  terminal_session_id: string;
  generation: number | null;
  broker_generation: number | null;
  broker_lease_epoch: number | null;
  broker_owner_presentation_id: string | null;
  state: "starting" | "running" | "exited" | "unavailable";
  content: string;
};

type LiveTerminalProps = Omit<ZellijAgentTerminalProps, "presentationId">;

type ZellijTerminalSlot = {
  agentId: string;
  node: HTMLElement;
  presentationId: string;
  props: LiveTerminalProps;
};

function slotCanOwnTerminal(slot: ZellijTerminalSlot | null | undefined): slot is ZellijTerminalSlot {
  return Boolean(
    slot
    && slot.props.visibility === "visible"
    && slot.props.renderState === "mounted"
    && slot.props.requestedInteraction === "interactive",
  );
}

type ZellijViewportRect = {
  left: number;
  top: number;
  width: number;
  height: number;
};

type ZellijPresentationStore = {
  activeAgentId: string | null;
  activeTargetId: string | null;
  activationSerial: number;
  brokerOwners: Map<string, ZellijBrokerObservation>;
  focusRequestSerial: number;
  slots: Map<string, ZellijTerminalSlot>;
  setBrokerOwner: (
    agentId: string,
    runtimeGeneration: number | null,
    leaseEpoch: number | null,
    presentationId: string | null,
  ) => void;
  upsertSlot: (targetId: string, slot: ZellijTerminalSlot) => void;
  removeSlot: (targetId: string) => void;
  activate: (agentId: string, targetId: string) => Promise<void>;
};

type ZellijBrokerObservation = {
  generation: number | null;
  leaseEpoch: number | null;
  owner: string | null;
};

function desktopMayOwnBroker(
  agentId: string,
  brokerOwners: Map<string, ZellijBrokerObservation>,
): boolean {
  const observation = brokerOwners.get(agentId);
  return observation === undefined
    || observation.owner === null
    || observation.owner === HABITAT_TERMINAL_PRESENTATION_ID;
}

export const useZellijPresentationStore = create<ZellijPresentationStore>((set, get) => {
  let activationQueue = Promise.resolve();
  let pendingTargetId: string | null = null;
  return {
    activeAgentId: null,
    activeTargetId: null,
    activationSerial: 0,
    brokerOwners: new Map(),
    focusRequestSerial: 0,
    slots: new Map(),
    setBrokerOwner: (agentId, generation, leaseEpoch, owner) => set((state) => {
      const existing = state.brokerOwners.get(agentId);
      if (
        existing?.generation !== null
        && existing?.generation !== undefined
        && generation !== null
        && generation < existing.generation
      ) return state;
      if (
        existing?.generation === generation
        && existing.leaseEpoch !== null
        && leaseEpoch !== null
        && leaseEpoch < existing.leaseEpoch
      ) return state;
      const brokerOwners = new Map(state.brokerOwners);
      brokerOwners.set(agentId, { generation, leaseEpoch, owner });
      return { brokerOwners };
    }),
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
      const invalidatesActivation = pendingTargetId === targetId;
      if (invalidatesActivation) pendingTargetId = null;
      if (state.activeTargetId !== targetId) {
        set({
          slots,
          ...(invalidatesActivation ? { activationSerial: state.activationSerial + 1 } : {}),
        });
        if (invalidatesActivation) {
          const reconciliation = activationQueue.then(async () => {
            const current = get();
            const active = current.activeTargetId
              ? current.slots.get(current.activeTargetId)
              : undefined;
            if (
              slotCanOwnTerminal(active)
              && desktopMayOwnBroker(active.agentId, current.brokerOwners)
            ) {
              await invoke<string>("activate_zellij_agent_terminal", {
                sessionId: active.agentId,
                brokerGeneration: current.brokerOwners.get(active.agentId)?.generation ?? null,
              });
            }
          });
          activationQueue = reconciliation.catch(() => undefined);
        }
        return;
      }
      const fallback = removed
        ? Array.from(slots.entries()).find(([, slot]) => slot.agentId === removed.agentId)?.[0]
        : undefined;
      set({
        slots,
        activeAgentId: fallback && removed ? removed.agentId : null,
        activeTargetId: fallback ?? null,
        ...(invalidatesActivation ? { activationSerial: state.activationSerial + 1 } : {}),
      });
    },
    activate: (agentId, targetId) => {
      const serial = get().activationSerial + 1;
      pendingTargetId = targetId;
      set({ activationSerial: serial });
      const activation = activationQueue.then(async () => {
        try {
          const slot = get().slots.get(targetId);
          if (
            get().activationSerial !== serial
            || !slotCanOwnTerminal(slot)
            || !desktopMayOwnBroker(slot.agentId, get().brokerOwners)
          ) return;
          await invoke<string>("activate_zellij_agent_terminal", {
            sessionId: agentId,
            brokerGeneration: get().brokerOwners.get(agentId)?.generation ?? null,
          });
          if (
            get().activationSerial === serial
            && slotCanOwnTerminal(get().slots.get(targetId))
            && desktopMayOwnBroker(agentId, get().brokerOwners)
          ) {
            set((state) => ({
              activeAgentId: agentId,
              activeTargetId: targetId,
              focusRequestSerial: state.focusRequestSerial + 1,
            }));
          }
        } finally {
          if (pendingTargetId === targetId) pendingTargetId = null;
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

function viewportRectFor(node: HTMLElement): ZellijViewportRect {
  const rect = node.getBoundingClientRect();
  let left = Math.max(0, rect.left);
  let top = Math.max(0, rect.top);
  let right = Math.min(window.innerWidth, rect.right);
  let bottom = Math.min(window.innerHeight, rect.bottom);
  let ancestor = node.parentElement;
  while (ancestor && ancestor !== document.body) {
    const style = window.getComputedStyle(ancestor);
    if (`${style.overflow} ${style.overflowX} ${style.overflowY}`.match(/auto|scroll|hidden|clip/)) {
      const bounds = ancestor.getBoundingClientRect();
      left = Math.max(left, bounds.left);
      top = Math.max(top, bounds.top);
      right = Math.min(right, bounds.right);
      bottom = Math.min(bottom, bounds.bottom);
    }
    ancestor = ancestor.parentElement;
  }
  return { left, top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) };
}

/** Mount once at app level. It is the only agent-facing xterm in the process. */
export function ZellijAgentTerminalHost() {
  const activeTargetId = useZellijPresentationStore((state) => state.activeTargetId);
  const target = useZellijPresentationStore((state) => {
    if (!state.activeTargetId) return null;
    return state.slots.get(state.activeTargetId) ?? null;
  });
  const [retainedTarget, setRetainedTarget] = useState<ZellijTerminalSlot | null>(null);
  const [rect, setRect] = useState<ZellijViewportRect | null>(null);
  const brokerOwners = useZellijPresentationStore((state) => state.brokerOwners);
  const setBrokerOwner = useZellijPresentationStore((state) => state.setBrokerOwner);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const retainedTargetIdRef = useRef<string | null>(null);
  const handledFocusRequestRef = useRef(0);
  const focusRequestSerial = useZellijPresentationStore((state) => state.focusRequestSerial);

  useLayoutEffect(() => {
    if (!target) {
      retainedTargetIdRef.current = null;
      return;
    }
    setRetainedTarget(target);
    if (retainedTargetIdRef.current !== activeTargetId) {
      retainedTargetIdRef.current = activeTargetId;
    }
  }, [activeTargetId, target]);

  useEffect(() => {
    if (
      !slotCanOwnTerminal(target)
      || !desktopMayOwnBroker(target.agentId, brokerOwners)
      || focusRequestSerial === 0
      || handledFocusRequestRef.current === focusRequestSerial
    ) return;
    let cancelled = false;
    let frame: number | null = null;
    const focusWhenReady = () => {
      if (cancelled) return true;
      const broker = useZellijPresentationStore.getState();
      if (!desktopMayOwnBroker(target.agentId, broker.brokerOwners)) {
        return true;
      }
      const terminalHost = viewportRef.current?.querySelector<HTMLElement>(
        '[data-testid="agent-terminal-host"]',
      );
      const helper = terminalHost?.querySelector<HTMLTextAreaElement>(
        ".xterm-helper-textarea",
      );
      if (
        terminalHost?.dataset.terminalSessionId !== target.agentId
        || !helper
        || window.getComputedStyle(terminalHost).visibility !== "visible"
      ) {
        return false;
      }
      helper.focus({ preventScroll: true });
      const focused = document.activeElement === helper;
      if (focused) handledFocusRequestRef.current = focusRequestSerial;
      return focused;
    };
    const observer = new MutationObserver(() => {
      if (focusWhenReady()) observer.disconnect();
    });
    if (!focusWhenReady() && viewportRef.current) {
      observer.observe(viewportRef.current, {
        attributes: true,
        attributeFilter: ["style", "data-terminal-session-id"],
        childList: true,
        subtree: true,
      });
      frame = window.requestAnimationFrame(() => {
        frame = null;
        if (focusWhenReady()) observer.disconnect();
      });
    }
    return () => {
      cancelled = true;
      observer.disconnect();
      if (frame !== null) window.cancelAnimationFrame(frame);
    };
  }, [brokerOwners, focusRequestSerial, target]);
  const renderedTarget = target ?? retainedTarget;

  useLayoutEffect(() => {
    if (!target) {
      setRect(null);
      return;
    }
    let frame: number | null = null;
    const update = () => {
      if (frame !== null) return;
      frame = window.requestAnimationFrame(() => {
        frame = null;
        setRect(viewportRectFor(target.node));
      });
    };
    update();
    const resizeObserver = new ResizeObserver(update);
    let observed: HTMLElement | null = target.node;
    while (observed && observed !== document.body) {
      resizeObserver.observe(observed);
      observed = observed.parentElement;
    }
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      if (frame !== null) window.cancelAnimationFrame(frame);
      resizeObserver.disconnect();
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [target]);

  if (!renderedTarget) return null;

  const props = renderedTarget.props;
  const relevantBrokerOwner = brokerOwners.get(renderedTarget.agentId)?.owner ?? null;
  const eligibleTarget = slotCanOwnTerminal(target)
    && desktopMayOwnBroker(target.agentId, brokerOwners);
  const effectiveInteraction = target
    && !desktopMayOwnBroker(target.agentId, brokerOwners)
    ? "read_only"
    : target ? props.requestedInteraction : "read_only";
  const visible = Boolean(eligibleTarget && rect && rect.width >= 10 && rect.height >= 10);
  return createPortal(
    <div
      ref={viewportRef}
      className="overflow-hidden bg-[var(--color-wardian-bg)]"
      data-zellij-agent-id={renderedTarget.agentId}
      data-terminal-broker-owner={relevantBrokerOwner ?? ""}
      data-zellij-presentation="renderer"
      data-zellij-singleton-viewport="true"
      style={{
        position: "fixed",
        left: visible ? rect?.left : -10_000,
        top: visible ? rect?.top : 0,
        width: visible ? rect?.width : 1,
        height: visible ? rect?.height : 1,
        visibility: visible ? "visible" : "hidden",
        pointerEvents: visible ? "auto" : "none",
        zIndex: 30,
      }}
    >
      <AgentTerminal
        sessionId={renderedTarget.agentId}
        presentationId={HABITAT_TERMINAL_PRESENTATION_ID}
        visibility={eligibleTarget ? props.visibility : "hidden"}
        renderState="mounted"
        requestedInteraction={effectiveInteraction}
        provider={props.provider}
        isMaximized={props.isMaximized}
        theme={props.theme}
        workspacePath={props.workspacePath}
        onTitleChange={props.onTitleChange}
        onTerminalFocus={props.onTerminalFocus}
        onPresentationStateChange={(brokerState, presentationState) => {
          setBrokerOwner(
            renderedTarget.agentId,
            brokerState.runtime_generation,
            brokerState.lease_epoch,
            brokerState.owner_presentation_id,
          );
          const singletonPresentationId = presentationState?.presentation_id
            ?? HABITAT_TERMINAL_PRESENTATION_ID;
          const localBrokerState = brokerState.owner_presentation_id
            === singletonPresentationId
            ? { ...brokerState, owner_presentation_id: renderedTarget.presentationId }
            : brokerState;
          const localPresentationState = presentationState
            ? { ...presentationState, presentation_id: renderedTarget.presentationId }
            : null;
          props.onPresentationStateChange?.(localBrokerState, localPresentationState);
        }}
        autoActivateWhenUnowned={eligibleTarget}
        autoFocus={props.autoFocus}
        lifetimeStableRenderer
      />
    </div>,
    document.body,
  );
}

/** A terminal slot. Only the selected slot receives the singleton host. */
export function ZellijAgentTerminal({ presentationId, ...props }: ZellijAgentTerminalProps) {
  const { sessionId, visibility, renderState, requestedInteraction, onTerminalFocus } = props;
  const targetId = `${presentationId}:${sessionId}`;
  const hostRef = useRef<HTMLElement | null>(null);
  const activeTargetId = useZellijPresentationStore((state) => state.activeTargetId);
  const brokerOwners = useZellijPresentationStore((state) => state.brokerOwners);
  const activate = useZellijPresentationStore((state) => state.activate);
  const setBrokerOwner = useZellijPresentationStore((state) => state.setBrokerOwner);
  const upsertSlot = useZellijPresentationStore((state) => state.upsertSlot);
  const removeSlot = useZellijPresentationStore((state) => state.removeSlot);
  const [preview, setPreview] = useState<ZellijTerminalPreview | null>(null);
  const [activationError, setActivationError] = useState<string | null>(null);
  const autoActivationAttempted = useRef(false);
  const activationRequested = useRef(false);
  const canOwnTerminal = visibility === "visible"
    && renderState === "mounted"
    && requestedInteraction === "interactive"
    && desktopMayOwnBroker(sessionId, brokerOwners);
  const isLiveTarget = activeTargetId === targetId && canOwnTerminal;

  useEffect(() => {
    const node = hostRef.current;
    if (node) upsertSlot(targetId, { agentId: sessionId, node, presentationId, props });
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
        if (!cancelled) {
          if (next.broker_generation !== null) {
            setBrokerOwner(
              sessionId,
              next.broker_generation,
              next.broker_lease_epoch,
              next.broker_owner_presentation_id,
            );
          }
          setPreview(next);
        }
      } catch {
        if (!cancelled) {
          setPreview({
            session_id: sessionId,
            terminal_session_id: sessionId,
            generation: null,
            broker_generation: null,
            broker_lease_epoch: null,
            broker_owner_presentation_id: null,
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
  }, [isLiveTarget, renderState, sessionId, setBrokerOwner, visibility]);

  useEffect(() => {
    if (isLiveTarget) activationRequested.current = false;
  }, [isLiveTarget]);

  const requestTerminalFocus = useCallback(() => {
    if (!canOwnTerminal || preview?.state !== "running" || activationRequested.current) return;
    activationRequested.current = true;
    setActivationError(null);
    onTerminalFocus?.();
    void activate(sessionId, targetId).catch((error) => {
      activationRequested.current = false;
      setActivationError(error instanceof Error ? error.message : "Terminal activation failed");
    });
  }, [activate, canOwnTerminal, onTerminalFocus, preview?.state, sessionId, targetId]);

  useEffect(() => {
    if (
      autoActivationAttempted.current
      || isLiveTarget
      || visibility !== "visible"
      || renderState !== "mounted"
      || requestedInteraction !== "interactive"
      || (!props.autoFocus && !props.autoActivateWhenUnowned)
    ) return;
    autoActivationAttempted.current = true;
    void activate(sessionId, targetId).catch((error) => {
      setActivationError(error instanceof Error ? error.message : "Terminal activation failed");
    });
  }, [activate, isLiveTarget, props.autoActivateWhenUnowned, props.autoFocus, renderState, requestedInteraction, sessionId, targetId, visibility]);

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
        : preview?.state === "running" ? null : "Starting terminal…");
  const interactive = canOwnTerminal && preview?.state === "running";

  return (
    <div
      ref={(node) => { hostRef.current = node; }}
      aria-disabled={!interactive}
      aria-label={`Terminal for ${sessionId}`}
      className="group relative flex h-full w-full min-h-0 min-w-0 flex-col overflow-hidden bg-[var(--color-wardian-bg)] text-left outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--color-wardian-accent)]"
      data-zellij-agent-id={sessionId}
      data-zellij-presentation="preview"
      data-testid={`zellij-terminal-preview-${sessionId}`}
      onFocus={requestTerminalFocus}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          requestTerminalFocus();
        }
      }}
      onPointerDown={requestTerminalFocus}
      role="application"
      tabIndex={interactive ? 0 : -1}
    >
      <pre aria-hidden="true" className="min-h-0 flex-1 overflow-hidden whitespace-pre-wrap p-2 font-mono text-[11px] leading-4 text-[var(--color-wardian-text-muted)]">
        {preview?.content || ""}
      </pre>
      {stateCopy ? (
        <span className="absolute inset-x-0 bottom-0 border-t border-wardian-border bg-[var(--color-wardian-sidebar-secondary)] px-2 py-1 text-center text-[11px] font-medium text-primary">
          {stateCopy}
        </span>
      ) : null}
    </div>
  );
}

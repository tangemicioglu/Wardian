import { invoke } from "@tauri-apps/api/core";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
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
import { terminalSessionClientFor } from "./terminalSessionClient";

export const HABITAT_TERMINAL_PRESENTATION_ID = "desktop:zellij-habitat-terminal";

type ZellijTerminalPreview = {
  session_id: string;
  terminal_session_id: string;
  generation: number | null;
  broker_generation: number | null;
  broker_lease_epoch: number | null;
  broker_owner_presentation_id: string | null;
  broker_activation_pending: boolean;
  state: "starting" | "running" | "exited" | "error" | "unavailable";
  content: string;
};

type LiveTerminalProps = Omit<ZellijAgentTerminalProps, "presentationId">;

type ZellijTerminalSlot = {
  agentId: string;
  node: HTMLElement;
  presentationId: string;
  props: LiveTerminalProps;
  terminalState: ZellijTerminalPreview["state"] | null;
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
  pendingInputByTarget: Map<string, string>;
  slots: Map<string, ZellijTerminalSlot>;
  clearPendingInput: (targetId: string) => void;
  consumePendingInput: (targetId: string, sentInput: string) => void;
  queuePendingInput: (targetId: string, input: string) => void;
  setBrokerOwner: (
    agentId: string,
    runtimeGeneration: number | null,
    leaseEpoch: number | null,
    presentationId: string | null,
    activationPending?: boolean,
    source?: "live" | "preview",
  ) => void;
  upsertSlot: (targetId: string, slot: ZellijTerminalSlot) => void;
  removeSlot: (targetId: string) => void;
  activate: (agentId: string, targetId: string) => Promise<boolean>;
};

type ZellijBrokerObservation = {
  generation: number | null;
  leaseEpoch: number | null;
  owner: string | null;
  activationPending: boolean;
  source: "live" | "preview";
};

let nextPreviewRequestToken = 0;
const latestPreviewRequestByAgent = new Map<string, number>();

function terminalInputForKeyDown(event: ReactKeyboardEvent<HTMLElement>): string | null {
  if (event.nativeEvent.isComposing || event.key === "Dead" || event.metaKey) return null;
  if (event.ctrlKey) {
    if (event.key === " ") return "\x00";
    if (event.key.length === 1) {
      const code = event.key.toUpperCase().charCodeAt(0);
      if (code >= 64 && code <= 95) {
        const control = String.fromCharCode(code & 0x1f);
        return event.altKey ? `\x1b${control}` : control;
      }
    }
    return null;
  }
  const named: Record<string, string> = {
    Enter: "\r",
    Backspace: "\x7f",
    Tab: "\t",
    Escape: "\x1b",
    ArrowUp: "\x1b[A",
    ArrowDown: "\x1b[B",
    ArrowRight: "\x1b[C",
    ArrowLeft: "\x1b[D",
    Home: "\x1b[H",
    End: "\x1b[F",
    Insert: "\x1b[2~",
    Delete: "\x1b[3~",
    PageUp: "\x1b[5~",
    PageDown: "\x1b[6~",
  };
  const input = named[event.key] ?? (event.key.length === 1 ? event.key : null);
  if (input === null) return null;
  return event.altKey ? `\x1b${input}` : input;
}

function beginPreviewRequest(agentId: string): number {
  const token = ++nextPreviewRequestToken;
  latestPreviewRequestByAgent.set(agentId, token);
  return token;
}

function isCurrentPreviewRequest(agentId: string, token: number): boolean {
  return latestPreviewRequestByAgent.get(agentId) === token;
}

function desktopMayOwnBroker(
  agentId: string,
  brokerOwners: Map<string, ZellijBrokerObservation>,
): boolean {
  const observation = brokerOwners.get(agentId);
  return observation === undefined
    || (!observation.activationPending && (
      observation.owner === null
      || observation.owner === HABITAT_TERMINAL_PRESENTATION_ID
    ));
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
    pendingInputByTarget: new Map(),
    slots: new Map(),
    clearPendingInput: (targetId) => set((state) => {
      if (!state.pendingInputByTarget.has(targetId)) return state;
      const pendingInputByTarget = new Map(state.pendingInputByTarget);
      pendingInputByTarget.delete(targetId);
      return { pendingInputByTarget };
    }),
    consumePendingInput: (targetId, sentInput) => set((state) => {
      const current = state.pendingInputByTarget.get(targetId) ?? "";
      if (!current.startsWith(sentInput)) return state;
      const pendingInputByTarget = new Map(state.pendingInputByTarget);
      const remaining = current.slice(sentInput.length);
      if (remaining) pendingInputByTarget.set(targetId, remaining);
      else pendingInputByTarget.delete(targetId);
      return { pendingInputByTarget };
    }),
    queuePendingInput: (targetId, input) => set((state) => {
      if (!input) return state;
      const pendingInputByTarget = new Map(state.pendingInputByTarget);
      pendingInputByTarget.set(
        targetId,
        `${pendingInputByTarget.get(targetId) ?? ""}${input}`,
      );
      return { pendingInputByTarget };
    }),
    setBrokerOwner: (
      agentId,
      generation,
      leaseEpoch,
      owner,
      activationPending = false,
      source = "live",
    ) => set((state) => {
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
      if (
        existing?.generation === generation
        && existing.leaseEpoch === leaseEpoch
        && existing.source === "live"
        && source === "preview"
        && !(existing.activationPending && !activationPending)
      ) return state;
      const brokerOwners = new Map(state.brokerOwners);
      brokerOwners.set(agentId, {
        generation,
        leaseEpoch,
        owner,
        activationPending,
        source,
      });
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
      const pendingInputByTarget = new Map(state.pendingInputByTarget);
      pendingInputByTarget.delete(targetId);
      if (removed && !Array.from(slots.values()).some((slot) => slot.agentId === removed.agentId)) {
        latestPreviewRequestByAgent.delete(removed.agentId);
      }
      const invalidatesActivation = pendingTargetId === targetId;
      if (invalidatesActivation) pendingTargetId = null;
      if (state.activeTargetId !== targetId) {
        set({
          slots,
          pendingInputByTarget,
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
        pendingInputByTarget,
        activeAgentId: fallback && removed ? removed.agentId : null,
        activeTargetId: fallback ?? null,
        ...(invalidatesActivation ? { activationSerial: state.activationSerial + 1 } : {}),
      });
    },
    activate: (agentId, targetId) => {
      const serial = get().activationSerial + 1;
      pendingTargetId = targetId;
      set({ activationSerial: serial });
      const activation = activationQueue.then(async (): Promise<boolean> => {
        try {
          const slot = get().slots.get(targetId);
          if (
            get().activationSerial !== serial
            || !slotCanOwnTerminal(slot)
            || !desktopMayOwnBroker(slot.agentId, get().brokerOwners)
          ) return false;
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
            return true;
          }
          return false;
        } finally {
          if (pendingTargetId === targetId) pendingTargetId = null;
        }
      });
      activationQueue = activation.then(() => undefined, () => undefined);
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
  const pendingInput = useZellijPresentationStore((state) => (
    activeTargetId ? state.pendingInputByTarget.get(activeTargetId) ?? "" : ""
  ));
  const consumePendingInput = useZellijPresentationStore((state) => state.consumePendingInput);
  const setBrokerOwner = useZellijPresentationStore((state) => state.setBrokerOwner);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const retainedTargetIdRef = useRef<string | null>(null);
  const handledFocusRequestRef = useRef(0);
  const pendingInputFlushTargetRef = useRef<string | null>(null);
  const [pendingInputRetrySerial, setPendingInputRetrySerial] = useState(0);
  const [pendingInputFlushStatus, setPendingInputFlushStatus] = useState(
    "idle" as "idle" | "sending" | "accepted" | "rejected" | "error",
  );
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
    let stableFocusFrames = 0;
    let remainingFocusFrames = 120;
    const focusWhenReady = () => {
      frame = null;
      if (cancelled) return;
      const broker = useZellijPresentationStore.getState();
      if (!desktopMayOwnBroker(target.agentId, broker.brokerOwners)) {
        stableFocusFrames = 0;
      } else {
        const terminalHost = viewportRef.current?.querySelector<HTMLElement>(
          '[data-testid="agent-terminal-host"]',
        );
        const helper = terminalHost?.querySelector<HTMLTextAreaElement>(
          ".xterm-helper-textarea",
        );
        if (
          terminalHost?.dataset.terminalSessionId === target.agentId
          && helper
          && window.getComputedStyle(terminalHost).visibility === "visible"
        ) {
          if (document.activeElement !== helper) {
            helper.focus({ preventScroll: true });
            stableFocusFrames = 0;
          }
          if (document.activeElement === helper) {
            stableFocusFrames += 1;
            if (stableFocusFrames >= 8) {
              handledFocusRequestRef.current = focusRequestSerial;
              return;
            }
          }
        } else {
          stableFocusFrames = 0;
        }
      }
      remainingFocusFrames -= 1;
      if (remainingFocusFrames > 0) {
        frame = window.requestAnimationFrame(focusWhenReady);
      }
    };
    frame = window.requestAnimationFrame(focusWhenReady);
    return () => {
      cancelled = true;
      if (frame !== null) window.cancelAnimationFrame(frame);
    };
  }, [brokerOwners, focusRequestSerial, target]);

  useEffect(() => {
    if (!target || !activeTargetId) return;
    const observation = brokerOwners.get(target.agentId);
    if (
      observation?.owner !== HABITAT_TERMINAL_PRESENTATION_ID
      || observation.source !== "live"
      || observation.activationPending
      || pendingInputFlushTargetRef.current === activeTargetId
    ) return;
    const input = pendingInput;
    if (!input) return;
    pendingInputFlushTargetRef.current = activeTargetId;
    setPendingInputFlushStatus("sending");
    void terminalSessionClientFor(target.agentId)
      .sendText(HABITAT_TERMINAL_PRESENTATION_ID, input)
      .then((decision) => {
        setPendingInputFlushStatus(decision.status);
        if (decision.status === "accepted") {
          consumePendingInput(activeTargetId, input);
        }
      })
      .catch(() => setPendingInputFlushStatus("error"))
      .finally(() => {
        if (pendingInputFlushTargetRef.current === activeTargetId) {
          pendingInputFlushTargetRef.current = null;
        }
        if (useZellijPresentationStore.getState().pendingInputByTarget.has(activeTargetId)) {
          window.setTimeout(() => setPendingInputRetrySerial((serial) => serial + 1), 50);
        }
      });
  }, [
    activeTargetId,
    brokerOwners,
    consumePendingInput,
    pendingInput,
    pendingInputRetrySerial,
    target,
  ]);

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
    const mutationObserver = new MutationObserver(() => {
      mutationObserver.disconnect();
      let ancestor: HTMLElement | null = target.node;
      while (ancestor) {
        mutationObserver.observe(ancestor, {
          attributes: true,
          attributeFilter: ["class", "style", "data-layout-revision"],
          childList: true,
        });
        if (ancestor === document.body) break;
        ancestor = ancestor.parentElement;
      }
      update();
    });
    let observed: HTMLElement | null = target.node;
    while (observed && observed !== document.body) {
      resizeObserver.observe(observed);
      observed = observed.parentElement;
    }
    let mutationAncestor: HTMLElement | null = target.node;
    while (mutationAncestor) {
      mutationObserver.observe(mutationAncestor, {
        attributes: true,
        attributeFilter: ["class", "style", "data-layout-revision"],
        childList: true,
      });
      if (mutationAncestor === document.body) break;
      mutationAncestor = mutationAncestor.parentElement;
    }
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      if (frame !== null) window.cancelAnimationFrame(frame);
      resizeObserver.disconnect();
      mutationObserver.disconnect();
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [target]);

  if (!renderedTarget) return null;

  const props = renderedTarget.props;
  const relevantBrokerOwner = brokerOwners.get(renderedTarget.agentId)?.owner ?? null;
  const eligibleTarget = slotCanOwnTerminal(target)
    && target.terminalState === "running"
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
      data-zellij-pending-input-length={pendingInput.length}
      data-zellij-pending-input-status={pendingInputFlushStatus}
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
            brokerState.pending_activation !== null,
            "live",
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
  const clearPendingInput = useZellijPresentationStore((state) => state.clearPendingInput);
  const queuePendingInput = useZellijPresentationStore((state) => state.queuePendingInput);
  const upsertSlot = useZellijPresentationStore((state) => state.upsertSlot);
  const removeSlot = useZellijPresentationStore((state) => state.removeSlot);
  const [preview, setPreview] = useState<ZellijTerminalPreview | null>(null);
  const [activationError, setActivationError] = useState<string | null>(null);
  const autoActivationAttempted = useRef(false);
  const activationRequested = useRef(false);
  const canOwnTerminal = visibility === "visible"
    && renderState === "mounted"
    && requestedInteraction === "interactive"
    && preview?.state === "running"
    && desktopMayOwnBroker(sessionId, brokerOwners);
  const isLiveTarget = activeTargetId === targetId && canOwnTerminal;

  useEffect(() => {
    const node = hostRef.current;
    if (node) {
      upsertSlot(targetId, {
        agentId: sessionId,
        node,
        presentationId,
        props,
        terminalState: preview?.state ?? null,
      });
    }
  });

  useEffect(() => () => removeSlot(targetId), [removeSlot, targetId]);

  useEffect(() => {
    if (visibility !== "visible" || renderState !== "mounted") return;
    let cancelled = false;
    const refresh = async () => {
      const requestToken = beginPreviewRequest(sessionId);
      try {
        const next = await invoke<ZellijTerminalPreview>("get_zellij_terminal_preview", {
          sessionId,
        });
        if (!cancelled) {
          if (isCurrentPreviewRequest(sessionId, requestToken)) {
            setBrokerOwner(
              sessionId,
              next.broker_generation,
              next.broker_lease_epoch,
              next.broker_owner_presentation_id,
              next.broker_activation_pending,
              "preview",
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
            broker_activation_pending: false,
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
  }, [renderState, sessionId, setBrokerOwner, visibility]);

  useEffect(() => {
    if (isLiveTarget) activationRequested.current = false;
  }, [isLiveTarget]);

  useEffect(() => {
    autoActivationAttempted.current = false;
  }, [canOwnTerminal, props.autoFocus, targetId]);

  const requestTerminalFocus = useCallback(() => {
    if (!canOwnTerminal || preview?.state !== "running" || activationRequested.current) return;
    activationRequested.current = true;
    setActivationError(null);
    onTerminalFocus?.();
    void activate(sessionId, targetId).then((committed) => {
      if (!committed) {
        activationRequested.current = false;
        clearPendingInput(targetId);
      }
    }).catch((error) => {
      activationRequested.current = false;
      clearPendingInput(targetId);
      setActivationError(error instanceof Error ? error.message : "Terminal activation failed");
    });
  }, [
    activate,
    canOwnTerminal,
    clearPendingInput,
    onTerminalFocus,
    preview?.state,
    sessionId,
    targetId,
  ]);

  useEffect(() => {
    if (
      autoActivationAttempted.current
      || isLiveTarget
      || !canOwnTerminal
      || !props.autoFocus
    ) return;
    autoActivationAttempted.current = true;
    void activate(sessionId, targetId).then((committed) => {
      if (!committed) autoActivationAttempted.current = false;
    }).catch((error) => {
      autoActivationAttempted.current = false;
      setActivationError(error instanceof Error ? error.message : "Terminal activation failed");
    });
  }, [
    activate,
    canOwnTerminal,
    isLiveTarget,
    props.autoFocus,
    sessionId,
    targetId,
  ]);

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
      : preview?.state === "error"
        ? "Terminal unavailable — restart the agent"
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
        const input = terminalInputForKeyDown(event);
        if (input === null) return;
        event.preventDefault();
        event.stopPropagation();
        queuePendingInput(targetId, input);
        requestTerminalFocus();
      }}
      onPaste={(event) => {
        const input = event.clipboardData.getData("text");
        if (!input) return;
        event.preventDefault();
        queuePendingInput(targetId, input);
        requestTerminalFocus();
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

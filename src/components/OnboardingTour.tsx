import { invoke } from "@tauri-apps/api/core";
import { Check, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useState } from "react";
import type { AgentConfig, TopologySnapshot } from "../types";
import type { WorkflowSchedule } from "../types/workflow";

interface OnboardingTourProps {
  agents: AgentConfig[];
  reviewMode: boolean;
  onClose: () => void;
  onComplete: () => void;
  onPrepareAgentCreation: () => void;
  onPrepareEvolver: (agent: AgentConfig) => void;
  onPrepareGraph: () => void;
  onPrepareWorkflow: () => void;
}

interface OnboardingWelcomeProps {
  onStart: () => void;
  onSkip: () => void;
}

type TourStepId = "create-evolver" | "create-orchestrator" | "connect-graph" | "schedule-review";

type TourStep = {
  id: TourStepId;
  title: string;
  detail: string;
  target: string;
};

const EVOLVER_PROMPT = "Use the Wardian CLI to inspect your own provider and workspace. Spawn an Orchestrator peer named orchestrator with the same provider and an explicit workspace. Do not use the Wardian home as its workspace and do not choose a model or effort unless I ask. Do not create a graph connection; I will connect the pair in Graph. Report the name and workspace you used.";

const STEPS: readonly TourStep[] = [
  {
    id: "create-evolver",
    title: "Create an Evolver",
    detail: "Give it a provider and a real workspace it can inspect, then spawn it. Wardian will wait here until the Evolver exists.",
    target: '[data-tour-target="spawn-agent-form"]',
  },
  {
    id: "create-orchestrator",
    title: "Let the Evolver create its partner",
    detail: "Paste this into the Evolver's terminal. It uses its Wardian CLI to spawn an Orchestrator with the same workspace. The tour advances when the Orchestrator appears in the roster.",
    target: '[data-tour-target="evolver-terminal"]',
  },
  {
    id: "connect-graph",
    title: "Connect the pair in Graph",
    detail: "Shift-drag between the Evolver and Orchestrator. The tour advances only after Wardian records their graph edge.",
    target: '[data-tour-target="graph-canvas"]',
  },
  {
    id: "schedule-review",
    title: "Schedule a conversation review",
    detail: "Open Conversation Pattern Review, choose Run, switch to Schedule, bind the evolver role, and choose a weekly cadence. The workflow only reports recommendations.",
    target: '[data-tour-target="workflow-view"]',
  },
] as const;

function agentWithClass(agents: AgentConfig[], agentClass: string): AgentConfig | undefined {
  return agents.find((agent) => agent.agent_class.trim().toLocaleLowerCase() === agentClass);
}

function nextStep(agents: AgentConfig[], linked: boolean, reviewScheduled: boolean): TourStepId | null {
  const evolver = agentWithClass(agents, "evolver");
  if (!evolver) return "create-evolver";
  if (!agentWithClass(agents, "orchestrator")) return "create-orchestrator";
  if (!linked) return "connect-graph";
  if (!reviewScheduled) return "schedule-review";
  return null;
}

/**
 * The first-launch decision is deliberately separate from the contextual
 * hints. It is only shown when the persisted onboarding state says that this
 * is a new habitat, and the choice is durable before a tour surface opens.
 */
export function OnboardingWelcome({ onStart, onSkip }: OnboardingWelcomeProps) {
  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/65 p-5" role="presentation">
      <section
        aria-labelledby="onboarding-welcome-title"
        aria-modal="true"
        className="w-full max-w-md rounded-xl border border-wardian-border bg-[var(--color-wardian-card)] p-6 shadow-2xl"
        data-testid="onboarding-welcome"
        role="dialog"
      >
        <p className="text-xs font-semibold text-[var(--color-wardian-accent)]">Welcome to Wardian</p>
        <h1 id="onboarding-welcome-title" className="mt-2 text-xl font-semibold text-primary">Build your first habitat</h1>
        <p className="mt-3 text-sm leading-6 text-muted">
          Take a short, hands-on tour that guides you through creating an Evolver, growing its Orchestrator partner, connecting them, and scheduling a review.
        </p>
        <div className="mt-6 flex flex-wrap justify-end gap-3">
          <button type="button" className="rounded-md px-3 py-2 text-sm font-medium text-muted transition-colors hover:text-primary" onClick={onSkip}>
            Not now
          </button>
          <button type="button" className="rounded-md bg-[var(--color-wardian-accent)] px-4 py-2 text-sm font-semibold text-[var(--color-wardian-bg)] transition-opacity hover:opacity-90" onClick={onStart}>
            Take the tour
          </button>
        </div>
      </section>
    </div>
  );
}

export function OnboardingTour({
  agents,
  reviewMode,
  onClose,
  onComplete,
  onPrepareAgentCreation,
  onPrepareEvolver,
  onPrepareGraph,
  onPrepareWorkflow,
}: OnboardingTourProps) {
  const evolver = useMemo(() => agentWithClass(agents, "evolver"), [agents]);
  const orchestrator = useMemo(() => agentWithClass(agents, "orchestrator"), [agents]);
  const [linked, setLinked] = useState(false);
  const [reviewScheduled, setReviewScheduled] = useState(false);
  const [reviewStepIndex, setReviewStepIndex] = useState(0);
  const activeStepId = reviewMode
    ? STEPS[reviewStepIndex]?.id ?? null
    : nextStep(agents, linked, reviewScheduled);
  const step = STEPS.find((candidate) => candidate.id === activeStepId) ?? null;

  useEffect(() => {
    if (reviewMode || !evolver || !orchestrator || activeStepId !== "connect-graph") return;
    let cancelled = false;
    const refresh = async () => {
      try {
        const topology = await invoke<TopologySnapshot>("get_topology");
        const edgeExists = topology.edges.some((edge) => (
          (edge.a === evolver.session_id && edge.b === orchestrator.session_id)
          || (edge.a === orchestrator.session_id && edge.b === evolver.session_id)
        ));
        if (!cancelled) setLinked(edgeExists);
      } catch {
        // The visible graph handles its own error presentation. Keep this guide non-invasive.
      }
    };
    void refresh();
    const intervalId = window.setInterval(() => { void refresh(); }, 1_000);
    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [activeStepId, evolver, orchestrator, reviewMode]);

  useEffect(() => {
    if (reviewMode || activeStepId !== "schedule-review") return;
    let cancelled = false;
    const refresh = async () => {
      try {
        const schedules = await invoke<WorkflowSchedule[]>("schedule_list");
        if (!cancelled) setReviewScheduled(schedules.some((schedule) => schedule.blueprint_id === "conversation-pattern-review"));
      } catch {
        // Scheduling remains usable even if the guide cannot observe this optional state.
      }
    };
    void refresh();
    const intervalId = window.setInterval(() => { void refresh(); }, 1_000);
    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [activeStepId, reviewMode]);

  useEffect(() => {
    if (activeStepId === "create-evolver") onPrepareAgentCreation();
    if (activeStepId === "create-orchestrator" && evolver) onPrepareEvolver(evolver);
    if (activeStepId === "connect-graph") onPrepareGraph();
    if (activeStepId === "schedule-review") onPrepareWorkflow();
  }, [activeStepId, evolver, onPrepareAgentCreation, onPrepareEvolver, onPrepareGraph, onPrepareWorkflow]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  if (!step) {
    return (
      <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/65 p-5" role="presentation">
        <section className="w-full max-w-sm rounded-xl border border-[var(--color-wardian-success)]/60 bg-[var(--color-wardian-card)] p-6 text-center shadow-2xl" data-testid="onboarding-tour-complete">
          <Check className="mx-auto h-8 w-8 text-[var(--color-wardian-success)]" aria-hidden="true" />
          <h2 className="mt-3 text-lg font-semibold text-primary">Habitat underway</h2>
          <p className="mt-2 text-sm leading-6 text-muted">Your first pair is connected and the conversation review is scheduled.</p>
          <button type="button" className="mt-5 rounded-md bg-[var(--color-wardian-accent)] px-4 py-2 text-sm font-semibold text-[var(--color-wardian-bg)]" onClick={onComplete}>
            Finish tour
          </button>
        </section>
      </div>
    );
  }

  return (
    <Spotlight
      step={step}
      reviewMode={reviewMode}
      onClose={onClose}
      onNext={() => {
        if (reviewStepIndex === STEPS.length - 1) {
          onClose();
          return;
        }
        setReviewStepIndex((index) => index + 1);
      }}
    />
  );
}

function Spotlight({
  step,
  reviewMode,
  onClose,
  onNext,
}: {
  step: TourStep;
  reviewMode: boolean;
  onClose: () => void;
  onNext: () => void;
}) {
  const [targetRect, setTargetRect] = useState<DOMRect | null>(null);

  useLayoutEffect(() => {
    let frameId = 0;
    let observer: ResizeObserver | null = null;
    let mutationObserver: MutationObserver | null = null;
    const update = () => {
      window.cancelAnimationFrame(frameId);
      frameId = window.requestAnimationFrame(() => {
        const target = document.querySelector<HTMLElement>(step.target);
        setTargetRect(target?.getBoundingClientRect() ?? null);
        observer?.disconnect();
        observer = target && typeof ResizeObserver !== "undefined" ? new ResizeObserver(update) : null;
        if (observer && target) observer.observe(target);
      });
    };
    update();
    mutationObserver = new MutationObserver(update);
    mutationObserver.observe(document.body, { childList: true, subtree: true });
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      window.cancelAnimationFrame(frameId);
      observer?.disconnect();
      mutationObserver?.disconnect();
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [step.target]);

  const padding = 10;
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;
  const top = Math.max(0, (targetRect?.top ?? 0) - padding);
  const left = Math.max(0, (targetRect?.left ?? 0) - padding);
  const right = Math.min(viewportWidth, (targetRect?.right ?? viewportWidth) + padding);
  const bottom = Math.min(viewportHeight, (targetRect?.bottom ?? viewportHeight) + padding);
  const tooltipTop = targetRect && bottom + 14 < viewportHeight - 220 ? bottom + 14 : Math.max(16, top - 190);
  const tooltipLeft = Math.min(Math.max(16, left), Math.max(16, viewportWidth - 380));

  return (
    <div className="pointer-events-none fixed inset-0 z-[100]" data-testid="onboarding-tour" role="presentation">
      {targetRect ? (
        <>
          <div className="pointer-events-auto fixed left-0 right-0 top-0 bg-black/65" style={{ height: top }} />
          <div className="pointer-events-auto fixed bottom-0 left-0 right-0 bg-black/65" style={{ top: bottom }} />
          <div className="pointer-events-auto fixed left-0 bg-black/65" style={{ top, height: bottom - top, width: left }} />
          <div className="pointer-events-auto fixed right-0 bg-black/65" style={{ top, height: bottom - top, left: right }} />
          <div className="pointer-events-none fixed rounded-lg border-2 border-[var(--color-wardian-accent)] shadow-[0_0_0_1px_color-mix(in_srgb,var(--color-wardian-bg),transparent_25%)]" style={{ top, left, width: right - left, height: bottom - top }} />
        </>
      ) : null}
      <section
        aria-live="polite"
        className="pointer-events-auto fixed z-[101] w-[min(360px,calc(100vw-2rem))] rounded-xl border border-wardian-border bg-[var(--color-wardian-card)] p-4 shadow-2xl"
        style={{ top: tooltipTop, left: tooltipLeft }}
      >
        <div className="flex items-start justify-between gap-3">
          <div>
            <p className="text-[11px] font-semibold text-[var(--color-wardian-accent)]">{reviewMode ? "Tour review" : "Guided setup"} · {STEPS.findIndex((candidate) => candidate.id === step.id) + 1} of {STEPS.length}</p>
            <h2 className="mt-1 text-sm font-semibold text-primary">{step.title}</h2>
          </div>
          <button aria-label="Exit guided tour" className="rounded p-1 text-muted-neutral transition-colors hover:bg-wardian-card-bg hover:text-primary" onClick={onClose} type="button">
            <X className="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
        <p className="mt-2 text-xs leading-5 text-muted">{step.detail}</p>
        {step.id === "create-orchestrator" ? (
          <code className="mt-3 block max-h-32 overflow-y-auto rounded border border-wardian-border bg-[var(--color-wardian-bg)] p-2 text-[10px] leading-4 text-primary">{EVOLVER_PROMPT}</code>
        ) : null}
        <div className="mt-3 flex items-center justify-between gap-3">
          <button className="text-xs font-medium text-muted underline-offset-2 hover:text-primary hover:underline" onClick={onClose} type="button">
            Exit tour
          </button>
          {reviewMode ? (
            <button className="rounded-md bg-[var(--color-wardian-accent)] px-3 py-1.5 text-xs font-semibold text-[var(--color-wardian-bg)] transition-opacity hover:opacity-90" onClick={onNext} type="button">
              {step.id === STEPS[STEPS.length - 1].id ? "Finish review" : "Next area"}
            </button>
          ) : null}
        </div>
      </section>
    </div>
  );
}

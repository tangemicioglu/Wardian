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

type TourStepId =
  | "evolver-name"
  | "evolver-class"
  | "evolver-workspace"
  | "evolver-provider"
  | "create-evolver"
  | "create-orchestrator"
  | "connect-graph"
  | "workflow-blueprint"
  | "workflow-run"
  | "workflow-schedule-mode"
  | "workflow-evolver-assignment"
  | "workflow-cadence"
  | "workflow-save-schedule";

type TourStep = {
  id: TourStepId;
  title: string;
  detail: string;
  target: string;
};

const EVOLVER_PROMPT = "Use the Wardian CLI to inspect your own provider and workspace. Spawn an Orchestrator peer named orchestrator with the same provider and the Wardian home workspace I configured for you. Do not choose a model or effort unless I ask. Do not create a graph connection; I will connect the pair in Graph. Report the name and workspace you used.";

const STEPS: readonly TourStep[] = [
  {
    id: "evolver-name",
    title: "Name your Evolver",
    detail: "Choose a short name that describes what this agent will improve. You can rename it later.",
    target: '[data-tour-target="spawn-agent-name"]',
  },
  {
    id: "evolver-class",
    title: "Choose the Evolver class",
    detail: "Select Evolver so this agent has a clear role in the habitat.",
    target: '[data-tour-target="spawn-agent-class"]',
  },
  {
    id: "evolver-workspace",
    title: "Choose its workspace",
    detail: "The Evolver manages your habitat from Wardian's own conversation archive, so use the Wardian home rather than a project folder.",
    target: '[data-tour-target="spawn-workspace-path"]',
  },
  {
    id: "evolver-provider",
    title: "Choose a provider",
    detail: "Use an installed provider. Leave model and effort at the provider default unless you need an explicit override.",
    target: '[data-tour-target="spawn-provider"]',
  },
  {
    id: "create-evolver",
    title: "Spawn the Evolver",
    detail: "Create it when the name, class, workspace, and provider look right. Wardian will wait here until the Evolver exists.",
    target: '[data-tour-target="spawn-submit"]',
  },
  {
    id: "create-orchestrator",
    title: "Ask the Evolver to create its partner",
    detail: "Copy this task, then click inside the Evolver terminal, paste, and send it. The Evolver uses its Wardian CLI to spawn an Orchestrator with the same workspace. The tour advances when the Orchestrator appears in the roster.",
    target: '[data-tour-target="evolver-terminal"]',
  },
  {
    id: "connect-graph",
    title: "Connect the pair in Graph",
    detail: "Shift-drag between the Evolver and Orchestrator. The tour advances only after Wardian records their graph edge.",
    target: '[data-tour-target="graph-canvas"]',
  },
  {
    id: "workflow-blueprint",
    title: "Open Conversation Pattern Review",
    detail: "Choose Conversation Pattern Review from the blueprint picker. It analyzes prior conversations and reports recommendations only.",
    target: '[data-tour-target="workflow-blueprint-selector"]',
  },
  {
    id: "workflow-run",
    title: "Open its launch settings",
    detail: "Choose Run. This opens the one-off and scheduled invocation settings for this workflow.",
    target: '[data-tour-target="workflow-run-button"]',
  },
  {
    id: "workflow-schedule-mode",
    title: "Switch to Schedule",
    detail: "In the launch settings, choose Schedule so this analysis will recur instead of running only once.",
    target: '[data-tour-target="workflow-schedule-mode"]',
  },
  {
    id: "workflow-evolver-assignment",
    title: "Assign the Evolver",
    detail: "For the evolver role, choose your Evolver agent. It will inspect past Wardian conversations without making changes.",
    target: '[data-tour-target="workflow-evolver-assignment"]',
  },
  {
    id: "workflow-cadence",
    title: "Choose a weekly cadence",
    detail: "Set the schedule to Weekly and choose the day and time that suits your habitat.",
    target: '[data-tour-target="workflow-schedule-editor"]',
  },
  {
    id: "workflow-save-schedule",
    title: "Save the review schedule",
    detail: "Save the schedule when the Evolver assignment and weekly cadence look right. Wardian will wait here until it exists.",
    target: '[data-tour-target="workflow-save-schedule"]',
  },
] as const;

const EVOLVER_SETUP_STEP_IDS = new Set<TourStepId>([
  "evolver-name",
  "evolver-class",
  "evolver-workspace",
  "evolver-provider",
  "create-evolver",
]);

const WORKFLOW_SETUP_STEP_IDS = new Set<TourStepId>([
  "workflow-blueprint",
  "workflow-run",
  "workflow-schedule-mode",
  "workflow-evolver-assignment",
  "workflow-cadence",
  "workflow-save-schedule",
]);

const EVOLVER_SETUP_STEPS = STEPS.filter((step) => EVOLVER_SETUP_STEP_IDS.has(step.id));
const WORKFLOW_SETUP_STEPS = STEPS.filter((step) => WORKFLOW_SETUP_STEP_IDS.has(step.id));

function agentWithClass(agents: AgentConfig[], agentClass: string): AgentConfig | undefined {
  return agents.find((agent) => agent.agent_class.trim().toLocaleLowerCase() === agentClass);
}

function nextStep(
  agents: AgentConfig[],
  linked: boolean,
  reviewScheduled: boolean,
  evolverSetupIndex: number,
  workflowSetupIndex: number,
): TourStepId | null {
  const evolver = agentWithClass(agents, "evolver");
  if (!evolver) return EVOLVER_SETUP_STEPS[evolverSetupIndex]?.id ?? "create-evolver";
  if (!agentWithClass(agents, "orchestrator")) return "create-orchestrator";
  if (!linked) return "connect-graph";
  if (!reviewScheduled) return WORKFLOW_SETUP_STEPS[workflowSetupIndex]?.id ?? "workflow-save-schedule";
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
  const [evolverSetupIndex, setEvolverSetupIndex] = useState(0);
  const [workflowSetupIndex, setWorkflowSetupIndex] = useState(0);
  const [wardianHome, setWardianHome] = useState<string | null>(null);
  const activeStepId = reviewMode
    ? STEPS[reviewStepIndex]?.id ?? null
    : nextStep(agents, linked, reviewScheduled, evolverSetupIndex, workflowSetupIndex);
  const step = STEPS.find((candidate) => candidate.id === activeStepId) ?? null;
  const canAdvanceSetup = Boolean(
    !reviewMode
    && !evolver
    && step
    && EVOLVER_SETUP_STEP_IDS.has(step.id)
    && step.id !== "create-evolver",
  );
  const canAdvanceWorkflow = Boolean(
    !reviewMode
    && evolver
    && orchestrator
    && linked
    && !reviewScheduled
    && step
    && WORKFLOW_SETUP_STEP_IDS.has(step.id)
    && step.id !== "workflow-save-schedule",
  );

  useEffect(() => {
    if (activeStepId !== "evolver-workspace") return;
    let cancelled = false;
    void Promise.resolve(invoke<string>("get_wardian_home_path"))
      .then((path) => {
        if (!cancelled) setWardianHome(typeof path === "string" ? path : null);
      })
      .catch(() => {
        if (!cancelled) setWardianHome(null);
      });
    return () => { cancelled = true; };
  }, [activeStepId]);

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
    if (reviewMode || activeStepId !== "workflow-save-schedule") return;
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
    if (activeStepId && EVOLVER_SETUP_STEP_IDS.has(activeStepId)) onPrepareAgentCreation();
    if (activeStepId === "create-orchestrator" && evolver) {
      onPrepareEvolver(evolver);
    }
    if (activeStepId === "connect-graph") onPrepareGraph();
    if (activeStepId && WORKFLOW_SETUP_STEP_IDS.has(activeStepId)) onPrepareWorkflow();
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
      wardianHome={wardianHome}
      onClose={onClose}
      showNext={reviewMode || canAdvanceSetup || canAdvanceWorkflow}
      nextLabel={reviewMode
        ? (step.id === STEPS[STEPS.length - 1].id ? "Finish review" : "Next area")
        : canAdvanceSetup ? "Next field" : "Next action"}
      onNext={() => {
        if (reviewMode) {
          if (reviewStepIndex === STEPS.length - 1) {
            onClose();
            return;
          }
          setReviewStepIndex((index) => index + 1);
          return;
        }
        if (canAdvanceSetup) {
          setEvolverSetupIndex((index) => index + 1);
          return;
        }
        setWorkflowSetupIndex((index) => index + 1);
      }}
    />
  );
}

function Spotlight({
  step,
  reviewMode,
  wardianHome,
  onClose,
  showNext,
  nextLabel,
  onNext,
}: {
  step: TourStep;
  reviewMode: boolean;
  wardianHome: string | null;
  onClose: () => void;
  showNext: boolean;
  nextLabel: string;
  onNext: () => void;
}) {
  const [targetRect, setTargetRect] = useState<DOMRect | null>(null);
  const [copiedPrompt, setCopiedPrompt] = useState(false);
  const [copiedWorkspace, setCopiedWorkspace] = useState(false);

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
  const tooltipWidth = Math.min(360, viewportWidth - 32);
  const tooltipHeight = step.id === "create-orchestrator" ? 330 : 210;
  const chromeOffset = 72;
  const candidates = [
    { top: chromeOffset, left: 16 },
    { top: chromeOffset, left: Math.max(16, viewportWidth - tooltipWidth - 16) },
    { top: Math.max(chromeOffset, viewportHeight - tooltipHeight - 16), left: 16 },
    { top: Math.max(chromeOffset, viewportHeight - tooltipHeight - 16), left: Math.max(16, viewportWidth - tooltipWidth - 16) },
  ];
  const tooltipPosition = candidates.reduce((best, candidate) => {
    if (!targetRect) return best;
    const overlapWidth = Math.max(0, Math.min(candidate.left + tooltipWidth, right) - Math.max(candidate.left, left));
    const overlapHeight = Math.max(0, Math.min(candidate.top + tooltipHeight, bottom) - Math.max(candidate.top, top));
    const overlap = overlapWidth * overlapHeight;
    const bestOverlapWidth = Math.max(0, Math.min(best.left + tooltipWidth, right) - Math.max(best.left, left));
    const bestOverlapHeight = Math.max(0, Math.min(best.top + tooltipHeight, bottom) - Math.max(best.top, top));
    return overlap < bestOverlapWidth * bestOverlapHeight ? candidate : best;
  }, candidates[0]);

  useEffect(() => {
    setCopiedPrompt(false);
    setCopiedWorkspace(false);
  }, [step.id]);

  const copyEvolverPrompt = async () => {
    try {
      await navigator.clipboard.writeText(EVOLVER_PROMPT);
      setCopiedPrompt(true);
    } catch {
      setCopiedPrompt(false);
    }
  };

  const copyWorkspacePath = async () => {
    if (!wardianHome) return;
    try {
      await navigator.clipboard.writeText(wardianHome);
      setCopiedWorkspace(true);
    } catch {
      setCopiedWorkspace(false);
    }
  };

  return (
    <div className="pointer-events-none fixed inset-0 z-[100]" data-testid="onboarding-tour" role="presentation">
      {targetRect ? (
        <>
          <div className="pointer-events-none fixed left-0 right-0 top-0 bg-black/65" style={{ height: top }} />
          <div className="pointer-events-none fixed bottom-0 left-0 right-0 bg-black/65" style={{ top: bottom }} />
          <div className="pointer-events-none fixed left-0 bg-black/65" style={{ top, height: bottom - top, width: left }} />
          <div className="pointer-events-none fixed right-0 bg-black/65" style={{ top, height: bottom - top, left: right }} />
          <div className="pointer-events-none fixed rounded-lg border-2 border-[var(--color-wardian-accent)] shadow-[0_0_0_1px_color-mix(in_srgb,var(--color-wardian-bg),transparent_25%)]" style={{ top, left, width: right - left, height: bottom - top }} />
        </>
      ) : null}
      <section
        aria-live="polite"
        className="pointer-events-auto fixed z-[101] w-[min(360px,calc(100vw-2rem))] rounded-xl border border-wardian-border bg-[var(--color-wardian-card)] p-4 shadow-2xl"
        style={tooltipPosition}
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
        {step.id === "evolver-workspace" ? (
          <>
            <code className="mt-3 block break-all rounded border border-wardian-border bg-[var(--color-wardian-bg)] p-2 text-[10px] leading-4 text-primary">
              {wardianHome ?? "Resolving your Wardian home…"}
            </code>
            <button className="mt-2 rounded border border-wardian-border px-2.5 py-1.5 text-xs font-medium text-primary hover:bg-wardian-card-bg disabled:opacity-50" disabled={!wardianHome} onClick={() => void copyWorkspacePath()} type="button">
              {copiedWorkspace ? "Copied — paste into Workspace Path" : "Copy Wardian home"}
            </button>
          </>
        ) : null}
        {step.id === "create-orchestrator" ? (
          <>
            <code className="mt-3 block max-h-28 overflow-y-auto rounded border border-wardian-border bg-[var(--color-wardian-bg)] p-2 text-[10px] leading-4 text-primary">{EVOLVER_PROMPT}</code>
            <button className="mt-2 rounded border border-wardian-border px-2.5 py-1.5 text-xs font-medium text-primary hover:bg-wardian-card-bg" onClick={() => void copyEvolverPrompt()} type="button">
              {copiedPrompt ? "Copied — paste into Evolver" : "Copy task"}
            </button>
          </>
        ) : null}
        <div className="mt-3 flex items-center justify-between gap-3">
          <button className="text-xs font-medium text-muted underline-offset-2 hover:text-primary hover:underline" onClick={onClose} type="button">
            Exit tour
          </button>
          {showNext ? (
            <button className="rounded-md bg-[var(--color-wardian-accent)] px-3 py-1.5 text-xs font-semibold text-[var(--color-wardian-bg)] transition-opacity hover:opacity-90" onClick={onNext} type="button">
              {nextLabel}
            </button>
          ) : null}
        </div>
      </section>
    </div>
  );
}

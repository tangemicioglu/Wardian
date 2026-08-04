import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AgentConfig } from "../types";
import { OnboardingTour, OnboardingWelcome } from "./OnboardingTour";

const evolver = {
  session_id: "evolver-id",
  session_name: "evolver",
  agent_class: "Evolver",
} as AgentConfig;

function tourProps(agents: AgentConfig[] = []) {
  return {
    agents,
    reviewMode: false,
    onClose: vi.fn(),
    onComplete: vi.fn(),
    onPrepareAgentCreation: vi.fn(),
    onPrepareEvolver: vi.fn(),
    onPrepareGraph: vi.fn(),
    onPrepareWorkflow: vi.fn(),
  };
}

describe("OnboardingTour", () => {
  it("guides each Evolver field before waiting for the spawn action", async () => {
    const user = userEvent.setup();
    const props = tourProps();
    const { rerender } = render(<OnboardingTour {...props} />);

    expect(screen.getByText("Name your Evolver")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next field" })).toBeInTheDocument();
    expect(props.onPrepareAgentCreation).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Next field" }));
    expect(screen.getByText("Choose the Evolver class")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next field" }));
    expect(screen.getByText("Choose its workspace")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next field" }));
    expect(screen.getByText("Choose a provider")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next field" }));
    expect(screen.getByText("Spawn the Evolver")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Next field" })).not.toBeInTheDocument();

    rerender(<OnboardingTour {...props} agents={[evolver]} />);

    expect(screen.getByText("Ask the Evolver to create its partner")).toBeInTheDocument();
    expect(screen.getByText(/Do not create a graph connection/)).toBeInTheDocument();
    expect(props.onPrepareEvolver).toHaveBeenCalledWith(evolver);
  });

  it("lets a user leave the action-gated tour", async () => {
    const user = userEvent.setup();
    const props = tourProps();
    render(<OnboardingTour {...props} />);

    await user.click(screen.getByRole("button", { name: "Exit guided tour" }));

    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it("replays every area from Settings instead of skipping to incomplete setup", async () => {
    const user = userEvent.setup();
    const props = tourProps([
      evolver,
      { ...evolver, session_id: "orchestrator-id", session_name: "orchestrator", agent_class: "Orchestrator" },
    ]);
    render(<OnboardingTour {...props} reviewMode />);

    expect(screen.getByText("Name your Evolver")).toBeInTheDocument();
    expect(screen.getByText("Tour review · 1 of 8")).toBeInTheDocument();
    expect(props.onPrepareAgentCreation).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Next area" }));

    expect(screen.getByText("Choose the Evolver class")).toBeInTheDocument();
  });

  it("does not intercept the focused target while it is being located", () => {
    const props = tourProps();
    render(<OnboardingTour {...props} />);

    expect(screen.getByTestId("onboarding-tour")).toHaveClass("pointer-events-none");
  });

  it("offers first-launch users an explicit choice", async () => {
    const user = userEvent.setup();
    const onStart = vi.fn();
    const onSkip = vi.fn();
    render(<OnboardingWelcome onStart={onStart} onSkip={onSkip} />);

    expect(screen.getByRole("dialog", { name: "Build your first habitat" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Take the tour" }));
    expect(onStart).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Not now" }));
    expect(onSkip).toHaveBeenCalledTimes(1);
  });
});

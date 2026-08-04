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
    onClose: vi.fn(),
    onComplete: vi.fn(),
    onPrepareAgentCreation: vi.fn(),
    onPrepareEvolver: vi.fn(),
    onPrepareGraph: vi.fn(),
    onPrepareWorkflow: vi.fn(),
  };
}

describe("OnboardingTour", () => {
  it("opens the real creation surface and waits for an Evolver before advancing", () => {
    const props = tourProps();
    const { rerender } = render(<OnboardingTour {...props} />);

    expect(screen.getByText("Create an Evolver")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Next" })).not.toBeInTheDocument();
    expect(props.onPrepareAgentCreation).toHaveBeenCalledTimes(1);

    rerender(<OnboardingTour {...props} agents={[evolver]} />);

    expect(screen.getByText("Let the Evolver create its partner")).toBeInTheDocument();
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

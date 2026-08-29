import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AutomationsSurface } from "./AutomationsSurface";

vi.mock("../../../views/AutomationsView", () => ({
  AutomationsView: () => <div data-testid="mock-automations-view" />,
}));

describe("AutomationsSurface", () => {
  it("hides the keep-alive automations renderer when the workbench surface is hidden", () => {
    render(<AutomationsSurface surface_id="automations-1" theme="light" visibility="hidden" />);

    const surface = screen.getByTestId("automations-surface");
    expect(surface).toHaveAttribute("aria-hidden", "true");
    expect(surface).toHaveAttribute("data-surface-visibility", "hidden");
    expect(surface).toHaveStyle({ display: "none" });
  });

  it("keeps the automations renderer visible when the workbench surface is active", () => {
    render(<AutomationsSurface surface_id="automations-1" theme="light" visibility="visible" />);

    const surface = screen.getByTestId("automations-surface");
    expect(surface).toHaveAttribute("aria-hidden", "false");
    expect(surface).toHaveAttribute("data-surface-visibility", "visible");
    expect(surface).not.toHaveStyle({ display: "none" });
    expect(screen.getByTestId("mock-automations-view")).toBeInTheDocument();
  });
});

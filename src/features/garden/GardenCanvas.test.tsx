import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("react-konva", () => ({
  Stage: ({ children }: any) => <div data-konva="stage">{children}</div>,
  Layer: ({ children }: any) => <div data-konva="layer">{children}</div>,
}));
vi.mock("./AgentUnit", () => ({
  AGENT_UNIT_NAME: "agent-unit",
  AgentUnit: ({ unit }: any) => <div data-testid="agent-unit">{unit.label}</div>,
}));
vi.mock("./WorkflowUnit", () => ({ WorkflowUnit: ({ unit }: any) => <div data-testid="workflow-unit">{unit.label}</div> }));

import { GardenCanvas } from "./GardenCanvas";

describe("GardenCanvas", () => {
  it("renders one node per agent and workflow unit", () => {
    render(
      <GardenCanvas
        agentUnits={[{ ref: { kind: "agent", id: "a1" }, label: "Alpha", status: "Idle", color: "#fff", position: { x: 0, y: 0 }, crown: [] }]}
        workflowUnits={[{ ref: { kind: "workflow", id: "w1" }, label: "Build", runStatus: "none", nodeCount: 1, position: { x: 0, y: 0 } }]}
        selectedKey={null}
        onSelect={vi.fn()}
        onOpenAgent={vi.fn()}
        onMoveUnit={vi.fn()}
        onResetLayout={vi.fn()}
      />,
    );
    expect(screen.getByTestId("agent-unit")).toHaveTextContent("Alpha");
    expect(screen.getByTestId("workflow-unit")).toHaveTextContent("Build");
  });

  describe("navigation affordances", () => {
    const renderCanvas = () =>
      render(
        <GardenCanvas
          agentUnits={[{ ref: { kind: "agent", id: "a1" }, label: "Alpha", status: "Idle", color: "#fff", position: { x: 0, y: 0 }, crown: [] }]}
          workflowUnits={[]}
          selectedKey={null}
          onSelect={vi.fn()}
          onOpenAgent={vi.fn()}
          onMoveUnit={vi.fn()}
          onResetLayout={vi.fn()}
        />,
      );

    it("shows how far out you are, and offers a way back", () => {
      // A canvas that looks empty gives no clue whether it is empty or whether
      // you are simply zoomed a long way out. The readout answers that, and Fit
      // is the one-click recovery.
      renderCanvas();
      expect(screen.getByTestId("garden-zoom-level")).toHaveTextContent("%");
      expect(screen.getByTestId("garden-fit-view")).toBeInTheDocument();
      expect(screen.getByLabelText("Zoom in")).toBeInTheDocument();
      expect(screen.getByLabelText("Zoom out")).toBeInTheDocument();
    });

    it("is focusable, so the keyboard can drive it", () => {
      // Without a tabIndex the map is reachable by mouse only — and the mouse is
      // exactly what was reported as confusing.
      const { container } = renderCanvas();
      expect(container.querySelector(".garden-canvas")).toHaveAttribute("tabindex", "0");
    });

    it("tells assistive tech which keys move the view", () => {
      renderCanvas();
      const label = screen.getByRole("img").getAttribute("aria-label") ?? "";
      expect(label).toMatch(/scroll to zoom/i);
      expect(label).toMatch(/arrow keys/i);
    });

    it("names the shortcut on each control", () => {
      renderCanvas();
      expect(screen.getByTestId("garden-fit-view")).toHaveAttribute("title", expect.stringContaining("0"));
      expect(screen.getByLabelText("Zoom in")).toHaveAttribute("title", expect.stringContaining("+"));
    });
  });
});

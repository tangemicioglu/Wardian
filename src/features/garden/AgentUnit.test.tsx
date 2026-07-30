import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

// Render Konva primitives as plain DOM for assertions.
vi.mock("react-konva", () => ({
  Group: ({ children, ...p }: any) => <div data-konva="group" {...filterProps(p)}>{children}</div>,
  Circle: (p: any) => <div data-konva="circle" data-fill={p.fill} data-stroke={p.stroke} />,
  Rect: (p: any) => <div data-konva="rect" data-fill={p.fill} />,
  Text: (p: any) => (
    <div data-konva="text" data-fill={p.fill} data-font={p.fontFamily} data-size={p.fontSize}>
      {p.text}
    </div>
  ),
}));

function filterProps(p: Record<string, unknown>) {
  // strip non-DOM props so React does not warn
  const { draggable, onDragMove, onDblClick, ...rest } = p;
  return rest;
}

import { AgentUnit } from "./AgentUnit";
import { CANVAS_LABEL_FONT, CANVAS_LABEL_SIZE } from "../../utils/canvasTypography";
import type { GardenTheme } from "./useGardenTheme";

const theme: GardenTheme = {
  label: "#111827",
  labelMuted: "#4b5563",
  selection: "#926a09",
  labelBackdrop: "#fcfaf5",
  font: CANVAS_LABEL_FONT,
  labelSize: CANVAS_LABEL_SIZE,
  subLabelSize: 10,
};

const unit = {
  ref: { kind: "agent" as const, id: "a1" },
  label: "Alpha",
  status: "Idle",
  color: "var(--color-wardian-success)",
  position: { x: 0, y: 0 },
};

describe("AgentUnit", () => {
  it("renders the agent label and a resolvable status fill", () => {
    document.documentElement.style.setProperty("--color-wardian-success", "#10b981");
    render(
      <AgentUnit
        unit={unit}
        selected={false}
        theme={theme}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
        onDragMove={vi.fn()}
      />,
    );
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    const circles = screen.getAllByText("", { selector: '[data-konva="circle"]' });
    expect(circles.some((c) => c.getAttribute("data-fill") === "#10b981")).toBe(true);
  });

  it("paints the label with the themed colour and the shared canvas font", () => {
    // The label previously hardcoded #cbd5e1 with no font family, so it was a
    // different typeface from every other pane and nearly invisible against the
    // light theme's background.
    render(
      <AgentUnit
        unit={unit}
        selected={false}
        theme={theme}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
        onDragMove={vi.fn()}
      />,
    );
    const label = screen.getByText("Alpha");
    expect(label).toHaveAttribute("data-fill", theme.label);
    expect(label).toHaveAttribute("data-font", CANVAS_LABEL_FONT);
    expect(label).toHaveAttribute("data-size", String(CANVAS_LABEL_SIZE));
  });

  it("draws the selection ring in the theme accent rather than hardcoded white", () => {
    // White is invisible on the default light background.
    render(
      <AgentUnit
        unit={unit}
        selected
        theme={theme}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
        onDragMove={vi.fn()}
      />,
    );
    const circles = screen.getAllByText("", { selector: '[data-konva="circle"]' });
    expect(circles.some((c) => c.getAttribute("data-stroke") === theme.selection)).toBe(true);
  });
});

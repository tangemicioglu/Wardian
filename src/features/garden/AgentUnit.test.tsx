import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

// Render Konva primitives as plain DOM for assertions.
vi.mock("react-konva", () => ({
  Group: ({ children, ...p }: any) => <div data-konva="group" {...filterProps(p)}>{children}</div>,
  Circle: (p: any) => (
    <div
      data-konva="circle"
      data-fill={p.fill}
      data-stroke={p.stroke}
      data-radius={p.radius}
      data-dash={p.dash ? "yes" : undefined}
    />
  ),
  Rect: (p: any) => <div data-konva="rect" data-fill={p.fill} />,
  Text: (p: any) => (
    <div data-konva="text" data-fill={p.fill} data-font={p.fontFamily} data-size={p.fontSize}>
      {p.text}
    </div>
  ),
}));

function filterProps(p: Record<string, unknown>) {
  // strip non-DOM props so React does not warn
  const { draggable, listening, onDragMove, onDblClick, onTap, onDblTap, ...rest } = p;
  return rest;
}

import { AgentUnit } from "./AgentUnit";
import { CANVAS_LABEL_FONT, CANVAS_LABEL_SIZE } from "../../utils/canvasTypography";
import type { GardenSkillGlyph } from "./skillGlyphs";
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

function glyph(overrides: Partial<GardenSkillGlyph> = {}): GardenSkillGlyph {
  return {
    entryRef: "skills/kicad",
    label: "KiCad Review",
    monogram: "KI",
    hue: 120,
    provenance: "direct",
    copied: false,
    ...overrides,
  };
}

const unit = {
  ref: { kind: "agent" as const, id: "a1" },
  label: "Alpha",
  status: "Idle",
  color: "var(--color-wardian-success)",
  position: { x: 0, y: 0 },
  crown: [],
};

function renderUnit(props: Partial<React.ComponentProps<typeof AgentUnit>> = {}) {
  return render(
    <AgentUnit
      unit={unit}
      selected={false}
      detail="near"
      theme={theme}
      onSelect={vi.fn()}
      onOpen={vi.fn()}
      onSelectSkill={vi.fn()}
      onOpenSkill={vi.fn()}
      onDragMove={vi.fn()}
      {...props}
    />,
  );
}

function circles() {
  return screen.getAllByText("", { selector: '[data-konva="circle"]' });
}

describe("AgentUnit", () => {
  it("renders the agent label and a resolvable status fill", () => {
    document.documentElement.style.setProperty("--color-wardian-success", "#10b981");
    renderUnit();
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(circles().some((c) => c.getAttribute("data-fill") === "#10b981")).toBe(true);
  });

  it("paints the label with the themed colour and the shared canvas font", () => {
    // The label previously hardcoded #cbd5e1 with no font family, so it was a
    // different typeface from every other pane and nearly invisible against the
    // light theme's background.
    renderUnit();
    const label = screen.getByText("Alpha");
    expect(label).toHaveAttribute("data-fill", theme.label);
    expect(label).toHaveAttribute("data-font", CANVAS_LABEL_FONT);
    expect(label).toHaveAttribute("data-size", String(CANVAS_LABEL_SIZE));
  });

  it("draws the selection ring in the theme accent rather than hardcoded white", () => {
    // White is invisible on the default light background.
    renderUnit({ selected: true });
    expect(circles().some((c) => c.getAttribute("data-stroke") === theme.selection)).toBe(true);
  });

  it("draws the skill crown so an agent's capabilities are visible on the map", () => {
    renderUnit({ unit: { ...unit, crown: [glyph()] } });
    expect(screen.getByText("KI")).toBeInTheDocument();
  });

  it("hides the crown at map scale, where glyphs would be texture not information", () => {
    renderUnit({ unit: { ...unit, crown: [glyph()] }, detail: "far" });
    expect(screen.queryByText("KI")).not.toBeInTheDocument();
  });

  it("truncates the crown and counts the remainder", () => {
    const crown = Array.from({ length: 9 }, (_, index) =>
      glyph({ entryRef: `skills/s${index}`, monogram: `S${index}` }),
    );
    renderUnit({ unit: { ...unit, crown }, detail: "mid" });
    expect(screen.getByText("S0")).toBeInTheDocument();
    expect(screen.queryByText("S6")).not.toBeInTheDocument();
    expect(screen.getByText("+3")).toBeInTheDocument();
  });

  it("dashes a copied deployment, which is a fork whose edits never sync back", () => {
    renderUnit({ unit: { ...unit, crown: [glyph({ copied: true })] } });
    expect(circles().some((c) => c.getAttribute("data-dash") === "yes")).toBe(true);
  });

  it("marks a carrier when a skill it holds is selected", () => {
    // The reverse index: instancing means "where is this skill used?" is
    // answered by a set of agents rather than by one position.
    const { container } = renderUnit({ unit: { ...unit, crown: [glyph()] }, highlighted: true });
    expect(
      [...container.querySelectorAll('[data-konva="circle"]')].some(
        (c) => c.getAttribute("data-stroke") === theme.selection && c.getAttribute("data-dash"),
      ),
    ).toBe(true);
  });
});

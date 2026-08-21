import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ProviderStrip } from "./ProviderStrip";
import { UNREPORTED } from "./telemetryFormat";
import type { FleetProviderRow, TelemetryFleetMaxima } from "./telemetryTypes";

function card(overrides: Partial<FleetProviderRow> = {}): FleetProviderRow {
  return {
    provider: "codex",
    roster_agent_count: 3,
    active_agent_count: 2,
    active_ms: 2_400_000,
    turns: 18,
    total_tokens: 120_000,
    files_touched: 12,
    lines_added: 340,
    lines_removed: 90,
    tokens_reported: true,
    spark: [0, 3, 9, 1],
    idle: false,
    ...overrides,
  };
}

const MAXIMA: TelemetryFleetMaxima = {
  tokens_per_hour: 0,
  turns_per_hour: 0,
  turns: 36,
  active_ms: 3_600_000,
  total_tokens: 240_000,
  files_touched: 24,
  lines: 860,
  spark: 9,
};

function renderStrip(
  providers: FleetProviderRow[],
  habitat: Partial<FleetProviderRow> = {},
) {
  return render(
    <ProviderStrip
      habitat={card({ provider: "all", roster_agent_count: 9, ...habitat })}
      providers={providers}
      maxima={MAXIMA}
      trendMeasure="total_tokens"
      grain="minute5"
    />,
  );
}

/** Every card, in the order the strip lays them out. */
function cardNames(): string[] {
  return screen
    .getAllByRole("region")
    .map((section) => within(section).getByRole("heading").textContent ?? "");
}

describe("ProviderStrip", () => {
  it("leads with the habitat, then the providers as ordered", () => {
    // Order is the backend's answer, not the strip's. Re-sorting here would let
    // the surface disagree with itself the moment the ordering rule changes.
    renderStrip([card({ provider: "claude" }), card({ provider: "codex" })]);

    expect(cardNames()).toEqual(["All", "Claude", "Codex"]);
  });

  it("scrolls horizontally rather than wrapping", () => {
    // Wrapping or shrinking would make the strip's height depend on how many
    // providers a habitat runs, which is the vendor-contingent layout this space
    // was held empty to avoid.
    renderStrip([card()]);

    const strip = screen.getByRole("group", { name: "Activity by provider" });
    expect(strip.className).toContain("overflow-x-auto");
    expect(strip.className).not.toContain("flex-wrap");
  });

  it("refuses to shrink, so a short surface cannot clip the cards", () => {
    // The strip's sibling is `flex-1` with basis 0%, so it cannot give up space
    // and the strip is what absorbs a short viewport. `overflow-x: auto` forces
    // `overflow-y` to compute as `auto`, so shrinking hides cards behind an
    // inner scrollbar rather than letting the surface scroll.
    renderStrip([card()]);

    const strip = screen.getByRole("group", { name: "Activity by provider" });
    expect(strip.className).toContain("flex-shrink-0");
  });

  it("renders an unreported token total as unreported, never as zero", () => {
    // A provider without token accounting has not been the thriftiest in the
    // habitat. Drawing it as 0 would rank it first on a measure it never took.
    renderStrip([card({ provider: "antigravity", total_tokens: null, tokens_reported: false })]);

    const antigravity = screen.getByRole("region", { name: /Antigravity/ });
    expect(within(antigravity).getByText(UNREPORTED)).toBeInTheDocument();
    expect(within(antigravity).queryByText("0")).not.toBeInTheDocument();
  });

  it("dims a provider that recorded nothing without dropping it", () => {
    // A configured provider you are not using is directly the answer to "where
    // can I spend what is left", which is the same reason idle agents stay
    // listed in the table below.
    renderStrip([card({ provider: "opencode", idle: true })]);

    const opencode = screen.getByRole("region", { name: /OpenCode/ });
    expect(opencode.className).toContain("opacity-40");
    expect(cardNames()).toContain("OpenCode");
  });

  it("prints an unrecognised provider as the store recorded it", () => {
    // The facts can name a provider this build has never heard of. Inventing a
    // label for it would be worse than showing what is actually stored.
    renderStrip([card({ provider: "some-new-vendor" })]);

    expect(cardNames()).toContain("some-new-vendor");
  });

  it("gives the line delta the full card width so removals cannot truncate", () => {
    // A half column fits about eight characters here, and a busy window renders
    // "+123.4k/-98.7k". Truncated, that reads as a figure with no removals —
    // a diverging measure silently turned into a wrong one.
    renderStrip([card({ lines_added: 123_400, lines_removed: 98_700 })]);

    const codex = screen.getByRole("region", { name: /Codex/ });
    const lines = within(codex).getByText("Lines").nextElementSibling;
    expect(lines).toHaveTextContent("+123.4k");
    expect(lines).toHaveTextContent("-98.7k");
    expect(lines?.parentElement?.className).toContain("col-span-2");
  });

  it("says the habitat sparkline is scaled to itself", () => {
    // It is the sum of the cards beside it, so a shared scale would peg it at
    // full height and flatten every provider onto the floor. The tooltip is what
    // keeps that from reading as a bug.
    renderStrip([card()], { spark: [4, 40, 8, 2] });

    const all = screen.getByRole("region", { name: /All/ });
    expect(within(all).getByRole("img")).toHaveAttribute(
      "title",
      expect.stringContaining("scaled to this card"),
    );
  });

  it("counts agents active in the window, not agents configured", () => {
    // Every other tile moves with the window. A roster count among them would be
    // the one figure that does not, which is a trap; it drives the ordering and
    // lives in the card's tooltip instead.
    // 77 appears in no other tile, so finding it would mean the roster count
    // leaked onto the card rather than colliding with some other figure.
    renderStrip([card({ provider: "codex", roster_agent_count: 77, active_agent_count: 2 })]);

    const codex = screen.getByRole("region", { name: /Codex/ });
    expect(within(codex).getByText("2 active")).toBeInTheDocument();
    expect(within(codex).queryByText(/77/)).not.toBeInTheDocument();
  });
});

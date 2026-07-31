import { describe, expect, it } from "vitest";
import { adjustTerminalForegroundContrast } from "./AgentsSharedTerminalSurface";

function relativeLuminance(hex: string) {
  const channels = [1, 3, 5].map((start) => Number.parseInt(hex.slice(start, start + 2), 16) / 255);
  const [red, green, blue] = channels.map((channel) => channel <= 0.03928
    ? channel / 12.92
    : ((channel + 0.055) / 1.055) ** 2.4);
  return red * 0.2126 + green * 0.7152 + blue * 0.0722;
}

function contrastRatio(first: string, second: string) {
  const firstLuminance = relativeLuminance(first);
  const secondLuminance = relativeLuminance(second);
  return (Math.max(firstLuminance, secondLuminance) + 0.05)
    / (Math.min(firstLuminance, secondLuminance) + 0.05);
}

describe("AgentsSharedTerminalSurface color fidelity", () => {
  it("raises low-contrast glyphs to xterm's configured contrast floor", () => {
    const adjusted = adjustTerminalForegroundContrast("#b8b8b8", "#fcfaf5", 4.5);

    expect(adjusted).not.toBe("#b8b8b8");
    expect(contrastRatio(adjusted, "#fcfaf5")).toBeGreaterThanOrEqual(4.5);
  });

  it("leaves already-legible colors unchanged", () => {
    expect(adjustTerminalForegroundContrast("#111827", "#fcfaf5", 4.5)).toBe("#111827");
  });

  it("leaves unsupported CSS color formats unchanged", () => {
    expect(adjustTerminalForegroundContrast("rgb(17, 24, 39)", "#fcfaf5", 4.5))
      .toBe("rgb(17, 24, 39)");
  });
});

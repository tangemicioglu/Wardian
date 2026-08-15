import { AppWindow } from "lucide-react";
import { describe, expect, it } from "vitest";

import { CORE_VIEW_SURFACE_DEFINITIONS } from "./surfaces/coreSurfaceMetadata";
import { mappedSurfaceIconTokens, surfaceIconForToken } from "./surfaceIcons";

describe("surface icons", () => {
  it("gives every core surface a glyph of its own", () => {
    // Analytics shipped without one. A surface definition's icon token is just
    // its type, so nothing connected the two tables, and the miss showed up as
    // a generic window tab rather than as an error.
    const missing = CORE_VIEW_SURFACE_DEFINITIONS.filter(
      (definition) => surfaceIconForToken(definition.icon) === AppWindow,
    ).map((definition) => definition.icon);

    expect(missing).toEqual([]);
  });

  it("distinguishes the surfaces that sit next to each other", () => {
    // Dashboard and Analytics are opened for different questions and are the
    // pair most easily confused in a crowded tab strip.
    expect(surfaceIconForToken("analytics")).not.toBe(surfaceIconForToken("dashboard"));
  });

  it("keeps the fallback reachable for genuinely unknown tokens", () => {
    expect(surfaceIconForToken("not-a-surface")).toBe(AppWindow);
    expect(mappedSurfaceIconTokens()).not.toContain("not-a-surface");
  });
});

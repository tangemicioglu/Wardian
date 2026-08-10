import { useEffect, useMemo, useState } from "react";
import {
  CANVAS_LABEL_FONT,
  CANVAS_LABEL_SIZE,
  CANVAS_SUBLABEL_SIZE,
} from "../../utils/canvasTypography";
import { clearResolvedColorCache, resolveCssVar } from "./resolveColor";
import type { TerrainChangeKind } from "./terrainPaint";

export interface GardenTheme {
  /** Primary label colour. */
  label: string;
  /** Secondary text — counts, kind tags. */
  labelMuted: string;
  /** Selection ring. */
  selection: string;
  /** Backdrop drawn behind label text so it stays legible over units. */
  labelBackdrop: string;
  /** Ground plane of a workspace root. */
  ground: string;
  /** Border between ground cells. */
  groundBorder: string;
  /** Fill for a directory cell below the root. */
  groundDir: string;
  /** Fill for a file cell. */
  groundFile: string;
  /**
   * Change tint per aggregate kind.
   *
   * Deliberately the same hues `ChangesPanel` uses for the same kinds: the map
   * and the pane describe one change set, and two palettes for one fact is a
   * thing the operator has to learn twice.
   */
  change: Record<TerrainChangeKind, string>;
  font: string;
  labelSize: number;
  subLabelSize: number;
}

/**
 * Resolve Garden canvas colours from the active theme.
 *
 * Konva paints concrete colour strings, so CSS custom properties have to be
 * resolved in JS — and re-resolved when the theme flips, or the canvas keeps
 * painting the previous theme's palette. `GraphCanvas` solves the same problem
 * the same way; the observer here exists for the same reason.
 *
 * Previously these were hardcoded (`#cbd5e1` labels, `#ffffff` selection),
 * which meant the Garden ignored the theme entirely. Both values are tuned for
 * a dark background, so in the default light theme labels were barely readable
 * and the selection ring was invisible.
 */
/**
 * The palette used when a CSS custom property cannot be read.
 *
 * Exported so canvas unit tests have one base to spread over rather than each
 * carrying its own literal: every new channel added here otherwise breaks every
 * fixture, which teaches people to stop adding channels.
 */
export const GARDEN_THEME_FALLBACK: GardenTheme = {
  label: "#111827",
  labelMuted: "#4b5563",
  selection: "#926a09",
  labelBackdrop: "#fcfaf5",
  ground: "#f5f1e8",
  groundBorder: "#e0d8c8",
  groundDir: "#efe9dc",
  groundFile: "#fbf8f1",
  change: {
    added: "#059669",
    modified: "#b45309",
    deleted: "#b91c1c",
    renamed: "#0891b2",
    untracked: "#059669",
    mixed: "#b45309",
  },
  font: CANVAS_LABEL_FONT,
  labelSize: CANVAS_LABEL_SIZE,
  subLabelSize: CANVAS_SUBLABEL_SIZE,
};

export function useGardenTheme(): GardenTheme {
  const [themeVersion, setThemeVersion] = useState(0);

  useEffect(() => {
    const observer = new MutationObserver(() => {
      // The resolver caches per theme name, which is correct for a theme swap.
      // A stylesheet edited *under* the same name would otherwise keep serving
      // stale colours, so drop the cache whenever the attribute is touched.
      clearResolvedColorCache();
      setThemeVersion((version) => version + 1);
    });
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    return () => observer.disconnect();
  }, []);

  // Stable identity per theme. Every canvas unit takes the theme as a prop, so
  // returning a fresh object each render would defeat their memoization and
  // re-render the whole map on every telemetry tick.
  return useMemo(
    () => ({
      label: resolveCssVar("var(--color-wardian-text)", GARDEN_THEME_FALLBACK.label),
      labelMuted: resolveCssVar(
        "var(--color-wardian-text-muted-neutral)",
        GARDEN_THEME_FALLBACK.labelMuted,
      ),
      selection: resolveCssVar("var(--color-wardian-accent)", GARDEN_THEME_FALLBACK.selection),
      labelBackdrop: resolveCssVar("var(--color-wardian-bg)", GARDEN_THEME_FALLBACK.labelBackdrop),
      ground: resolveCssVar("var(--color-wardian-card)", GARDEN_THEME_FALLBACK.ground),
      groundBorder: resolveCssVar(
        "var(--color-wardian-border)",
        GARDEN_THEME_FALLBACK.groundBorder,
      ),
      groundDir: resolveCssVar(
        "var(--color-wardian-card-bg-muted)",
        GARDEN_THEME_FALLBACK.groundDir,
      ),
      groundFile: resolveCssVar("var(--color-wardian-input-bg)", GARDEN_THEME_FALLBACK.groundFile),
      change: {
        added: resolveCssVar("var(--color-wardian-success)", GARDEN_THEME_FALLBACK.change.added),
        modified: resolveCssVar(
          "var(--color-wardian-warning)",
          GARDEN_THEME_FALLBACK.change.modified,
        ),
        deleted: resolveCssVar("var(--color-wardian-error)", GARDEN_THEME_FALLBACK.change.deleted),
        renamed: resolveCssVar(
          "var(--color-wardian-processing)",
          GARDEN_THEME_FALLBACK.change.renamed,
        ),
        untracked: resolveCssVar(
          "var(--color-wardian-success)",
          GARDEN_THEME_FALLBACK.change.untracked,
        ),
        mixed: resolveCssVar("var(--color-wardian-warning)", GARDEN_THEME_FALLBACK.change.mixed),
      },
      font: CANVAS_LABEL_FONT,
      labelSize: CANVAS_LABEL_SIZE,
      subLabelSize: CANVAS_SUBLABEL_SIZE,
    }),
    // themeVersion is the invalidation signal; the values come from the DOM.
    [themeVersion],
  );
}

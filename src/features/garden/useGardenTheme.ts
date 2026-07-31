import { useEffect, useMemo, useState } from "react";
import {
  CANVAS_LABEL_FONT,
  CANVAS_LABEL_SIZE,
  CANVAS_SUBLABEL_SIZE,
} from "../../utils/canvasTypography";
import { clearResolvedColorCache, resolveCssVar } from "./resolveColor";

export interface GardenTheme {
  /** Primary label colour. */
  label: string;
  /** Secondary text — counts, kind tags. */
  labelMuted: string;
  /** Selection ring. */
  selection: string;
  /** Backdrop drawn behind label text so it stays legible over units. */
  labelBackdrop: string;
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
      label: resolveCssVar("var(--color-wardian-text)", "#111827"),
      labelMuted: resolveCssVar("var(--color-wardian-text-muted-neutral)", "#4b5563"),
      selection: resolveCssVar("var(--color-wardian-accent)", "#926a09"),
      labelBackdrop: resolveCssVar("var(--color-wardian-bg)", "#fcfaf5"),
      font: CANVAS_LABEL_FONT,
      labelSize: CANVAS_LABEL_SIZE,
      subLabelSize: CANVAS_SUBLABEL_SIZE,
    }),
    // themeVersion is the invalidation signal; the values come from the DOM.
    [themeVersion],
  );
}

const VAR_PATTERN = /var\((--[^),]+)\)/;

/**
 * Resolved colours, keyed by `<theme>|<expression>`.
 *
 * `getComputedStyle` forces a style recalculation, and this is called once per
 * unit per render — with a full roster that is dozens of synchronous layout
 * reads on every telemetry tick, which was a material part of the Garden's
 * frame cost. The palette only changes when the theme does, so the theme
 * attribute is part of the key and the cache needs no explicit invalidation:
 * reading one attribute is free, unlike reading a computed style.
 */
const cache = new Map<string, string>();

function activeTheme(): string {
  if (typeof document === "undefined") return "";
  return document.documentElement.getAttribute("data-theme") ?? "";
}

/** Resolve a CSS custom property to a concrete color so Konva can paint it. */
export function resolveCssVar(value: string, fallback = "#94a3b8"): string {
  const match = value.match(VAR_PATTERN);
  if (!match) return value;
  if (typeof document === "undefined") return fallback;

  const key = `${activeTheme()}|${value}`;
  const cached = cache.get(key);
  if (cached !== undefined) return cached;

  const resolved =
    getComputedStyle(document.documentElement).getPropertyValue(match[1]).trim() || fallback;
  cache.set(key, resolved);
  return resolved;
}

/**
 * Drop every cached colour.
 *
 * Needed only when the stylesheet itself changes under a theme name — a live
 * theme edit, or a test swapping custom properties on the same document.
 */
export function clearResolvedColorCache(): void {
  cache.clear();
}

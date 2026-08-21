import { cellIntensity } from "./telemetryFormat";

/**
 * One row's trend across a window.
 *
 * Lives in the feature rather than beside its first caller because the Dashboard
 * now draws two of them at different granularities — one per agent row, one per
 * provider card — and they must agree on shape, or the strip and the table would
 * appear to disagree about the same window.
 */
export function Sparkline({
  values,
  max,
  label,
  title,
}: {
  values: readonly number[];
  max: number;
  label: string;
  /** Overrides the hover text, e.g. to say a card is scaled to itself. */
  title?: string;
}) {
  if (values.length === 0) return <span className="h-5" aria-hidden="true" />;
  const peak = values.reduce((highest, value) => Math.max(highest, value), 0);

  return (
    <span
      className="dashboard-view__spark flex items-end gap-px h-5"
      role="img"
      title={title}
      aria-label={peak > 0 ? `${label}: trend across the window` : `${label}: nothing recorded`}
    >
      {values.map((value, index) => (
        <span
          key={index}
          className={`flex-1 min-w-px rounded-t-[1px] ${
            value > 0 ? "bg-[var(--color-wardian-accent)]" : "bg-wardian-border/30"
          }`}
          // Square-rooted, not linear. Token rates span orders of magnitude
          // across a habitat, so a linear ratio against the busiest bucket
          // flattens every ordinary one onto the floor and the sparkline
          // degenerates into a dotted line with a single spike — the shape it
          // exists to show is exactly what gets lost.
          style={{ height: value > 0 ? `${Math.max(8, cellIntensity(value, max) * 100)}%` : "1px" }}
        />
      ))}
    </span>
  );
}

/**
 * The Dashboard's column registry.
 *
 * Columns are user-selected, because Wardian is malleable software. That does
 * not make the default arbitrary: the default set is the product's opinion about
 * what matters on a fleet monitor, and most operators will never change it.
 *
 * Every column declares its `kind`, which is load-bearing rather than
 * decorative. The Dashboard reads one trailing *window*, and that window changes
 * what a total means while leaving a rate untouched. Without the distinction an
 * operator can put "Tokens" and "Tokens/hr" side by side, watch them disagree,
 * and reasonably conclude the surface is broken.
 */

import type { FleetRow, TelemetryFleetMaxima } from "./telemetryTypes";

/**
 * What kind of quantity a column carries.
 *
 * - `identity` — who the row is. No window.
 * - `instant` — live right now, read from agent state rather than the telemetry
 *   store. Has no window either, and can disagree with stored figures at the
 *   edges because it is measured at a different moment.
 * - `rate` — per unit time. Unchanged when the window changes.
 * - `total` — summed over the window. **Changes meaning** with the window, so
 *   its header says which window it covers.
 * - `trend` — the shape of a measure across the window's buckets.
 */
export type ColumnKind = "identity" | "instant" | "rate" | "total" | "trend";

/** How a cell draws its value beside the number. */
export type ColumnVisual = "none" | "bar" | "spark" | "dot" | "diverging";

export interface DashboardColumn {
  id: string;
  label: string;
  kind: ColumnKind;
  visual: ColumnVisual;
  /** Whether clicking the header sorts by this column. */
  sortable: boolean;
  /** Column width as a CSS grid track. */
  width: string;
  /** One-line explanation, shown in the picker. */
  hint: string;
}

/**
 * Every column the Dashboard can show, in display order.
 *
 * This list is authoritative for which columns exist and where they sit. Saved
 * preferences supply visibility only, so a column added here appears for
 * existing users with no migration, and a stale preferences file cannot hide it.
 *
 * The order here is also the order the picker offers and the table draws, so
 * `DEFAULT_DASHBOARD_PREFS.columns` must list the same ids in the same sequence.
 * They drifted once — the table read prefs order while the picker read this one,
 * and a reader toggling a checkbox got a column somewhere other than where the
 * checkbox sat. A test pins them together.
 */
export const DASHBOARD_COLUMNS: readonly DashboardColumn[] = [
  {
    id: "state",
    label: "",
    kind: "instant",
    visual: "dot",
    sortable: false,
    width: "12px",
    hint: "Live status: idle, processing, action required, error, off",
  },
  {
    id: "agent",
    label: "Agent",
    kind: "identity",
    visual: "none",
    sortable: true,
    width: "minmax(140px, 1.4fr)",
    hint: "Agent name and class",
  },
  {
    id: "trend",
    label: "Trend",
    kind: "trend",
    visual: "spark",
    sortable: false,
    width: "minmax(90px, 1.2fr)",
    hint: "The sorted measure across the window",
  },
  {
    id: "active",
    label: "Active",
    kind: "total",
    visual: "bar",
    sortable: true,
    width: "minmax(84px, 0.7fr)",
    hint: "Time the agent was working in the window (inferred for most providers)",
  },
  {
    id: "turns",
    label: "Turns",
    kind: "total",
    visual: "bar",
    sortable: true,
    width: "minmax(72px, 0.6fr)",
    hint: "Turns taken in the window",
  },
  {
    id: "tokens",
    label: "Tokens",
    kind: "total",
    visual: "bar",
    sortable: true,
    width: "minmax(88px, 0.7fr)",
    hint: "Billable tokens in the window — fresh input plus output",
  },
  {
    id: "files",
    label: "Files",
    kind: "total",
    visual: "bar",
    sortable: true,
    width: "minmax(72px, 0.6fr)",
    hint: "Distinct files touched in the window",
  },
  {
    id: "lines",
    label: "Lines",
    kind: "total",
    visual: "diverging",
    sortable: true,
    width: "minmax(96px, 0.8fr)",
    hint: "Lines added against lines removed",
  },
  {
    id: "tokens_per_hour",
    label: "Tokens/hr",
    kind: "rate",
    visual: "bar",
    sortable: true,
    width: "minmax(96px, 0.9fr)",
    hint: "Billable tokens per hour, for comparing pace rather than volume",
  },
  {
    id: "turns_per_hour",
    label: "Turns/hr",
    kind: "rate",
    visual: "bar",
    sortable: true,
    width: "minmax(84px, 0.7fr)",
    hint: "Turns per hour, for comparing pace rather than volume",
  },
  {
    id: "cpu",
    label: "CPU",
    kind: "instant",
    visual: "bar",
    sortable: true,
    width: "minmax(72px, 0.6fr)",
    hint: "Live processor use across this agent's process tree",
  },
  {
    id: "memory",
    label: "Memory",
    kind: "instant",
    visual: "bar",
    sortable: true,
    width: "minmax(80px, 0.6fr)",
    hint: "Live resident memory across this agent's process tree",
  },
] as const;

export interface DashboardColumnPref {
  id: string;
  visible: boolean;
}

export interface DashboardPrefs {
  /** Which generation of {@link DEFAULT_DASHBOARD_PREFS} this was written against. */
  version: number;
  columns: DashboardColumnPref[];
  /** Trailing window in minutes. */
  window_minutes: number;
  sort: { column_id: string; descending: boolean };
}

/**
 * The generation of the default column set.
 *
 * **Bump this whenever the *default visibility* of a column changes**, which is
 * to say whenever the product changes its mind about what a Dashboard should
 * show out of the box. Adding a column without disturbing the existing set does
 * not need a bump — the default-anchored merge already handles that case, which
 * is the case it was designed for.
 *
 * It exists because that merge could not express this one. Saved visibility wins
 * for every id that still exists, so a revised default reached only operators
 * with no preferences file: everyone already running kept the old opinion
 * forever, with no way to tell that from a deliberate choice.
 *
 * 1. `state · agent · trend · burn · throughput · files · lines` — rates.
 * 2. `state · agent · trend · active · turns · tokens · files · lines` — totals,
 *    with both rate columns available and off.
 */
export const DASHBOARD_PREFS_VERSION = 2;

/**
 * The opinion.
 *
 * Totals, not rates. Rates were tried as the default and read badly on the
 * windows people actually use: over a day, real work collapses into figures like
 * `0.2/h` and `1.7/h`, which are arithmetically true and say nothing. "28 turns"
 * and "1d 8h active" are quantities an operator can hold. The two rate columns
 * stay available for anyone who wants that view, named for what they carry:
 * "Tokens/hr" and "Turns/hr". They were "Burn" and "Throughput", which named a
 * judgment rather than a unit — a reader had to already know that burn meant
 * tokens and throughput meant turns.
 *
 * Output still sits beside consumption, so the runaway remains visible by
 * sorting: an agent with the most tokens and no files touched is spinning, and
 * that reads the same in totals as it did in rates.
 *
 * CPU and memory are available but off. They carry real ceilings, which would
 * make honest denominators, but they are rarely what an operator is watching.
 */
export const DEFAULT_DASHBOARD_PREFS: DashboardPrefs = {
  version: DASHBOARD_PREFS_VERSION,
  columns: [
    { id: "state", visible: true },
    { id: "agent", visible: true },
    { id: "trend", visible: true },
    { id: "active", visible: true },
    { id: "turns", visible: true },
    { id: "tokens", visible: true },
    { id: "files", visible: true },
    { id: "lines", visible: true },
    // Off by default. Order still mirrors DASHBOARD_COLUMNS, so switching one on
    // places it where its checkbox sits.
    { id: "tokens_per_hour", visible: false },
    { id: "turns_per_hour", visible: false },
    { id: "cpu", visible: false },
    { id: "memory", visible: false },
  ],
  window_minutes: 1440,
  sort: { column_id: "active", descending: true },
};

/**
 * Merge saved preferences over the defaults.
 *
 * The default list drives the result, so a column introduced after a preferences
 * file was written still appears, in its intended position. Saved entries
 * contribute visibility and nothing else.
 *
 * **Unless the default set itself has changed.** A file written against an
 * earlier {@link DASHBOARD_PREFS_VERSION} has its column choices discarded and
 * takes the current defaults. Without that, a revised default reached nobody who
 * was already running: every id that still existed kept whatever visibility the
 * old opinion gave it, so an operator upgrading from the rates default kept
 * seeing rates and never saw Active or Tokens at all.
 *
 * The window survives the reset. It is an orthogonal choice — "show me a week"
 * says nothing about which columns belong on a Dashboard — and re-picking it
 * every time the column set is revised would be friction for no gain.
 */
export function mergeDashboardPrefs(saved: unknown): DashboardPrefs {
  if (!saved || typeof saved !== "object") return DEFAULT_DASHBOARD_PREFS;
  const record = saved as Partial<DashboardPrefs>;

  const windowOnly = record.version !== DASHBOARD_PREFS_VERSION;

  const savedColumns = new Map(
    (Array.isArray(record.columns) ? record.columns : [])
      .filter((column): column is DashboardColumnPref =>
        Boolean(column) && typeof column.id === "string" && typeof column.visible === "boolean",
      )
      .map((column) => [column.id, column]),
  );

  const windowMinutes =
    typeof record.window_minutes === "number" && Number.isFinite(record.window_minutes)
      ? clampWindow(record.window_minutes)
      : DEFAULT_DASHBOARD_PREFS.window_minutes;

  // A sort naming a column the new default set hides would order the table by
  // something the reader cannot see, so it resets with the columns.
  const sort =
    !windowOnly &&
    record.sort &&
    typeof record.sort.column_id === "string" &&
    typeof record.sort.descending === "boolean" &&
    DASHBOARD_COLUMNS.some((column) => column.id === record.sort?.column_id)
      ? record.sort
      : DEFAULT_DASHBOARD_PREFS.sort;

  return {
    version: DASHBOARD_PREFS_VERSION,
    columns: windowOnly
      ? [...DEFAULT_DASHBOARD_PREFS.columns]
      : DEFAULT_DASHBOARD_PREFS.columns.map(
          (fallback) => savedColumns.get(fallback.id) ?? fallback,
        ),
    window_minutes: windowMinutes,
    sort,
  };
}

/**
 * Windows the picker offers, in minutes.
 *
 * Reaching past a day is not a category error here. A fleet monitor is still
 * answering "what has this habitat been doing", and an agent that ran hard three
 * days ago is part of that answer; capping at 24 hours simply hid it. Analytics
 * still owns arbitrary horizons and the axis you read values off.
 */
export const WINDOW_CHOICES: readonly { minutes: number; label: string }[] = [
  { minutes: 15, label: "15 min" },
  { minutes: 60, label: "1 hour" },
  { minutes: 360, label: "6 hours" },
  { minutes: 1440, label: "24 hours" },
  { minutes: 10_080, label: "7 days" },
  { minutes: 43_200, label: "30 days" },
] as const;

/** The widest window the backend accepts, in minutes. */
export const MAX_WINDOW_MINUTES = 90 * 24 * 60;

/** Bounds mirroring the backend clamp, so the UI cannot ask for a rejected window. */
export function clampWindow(minutes: number): number {
  return Math.min(Math.max(Math.round(minutes), 5), MAX_WINDOW_MINUTES);
}

/** The measure whose shape the trend column draws, given the sorted column. */
export function trendMeasureFor(columnId: string): string {
  switch (columnId) {
    case "turns_per_hour":
    case "turns":
      return "turns";
    case "files":
      return "files";
    case "lines":
      return "lines_changed";
    case "active":
      return "active_ms";
    // Tokens/hr and Tokens are the same quantity at different denominations, so
    // both draw the same shape; only the number beside it differs.
    default:
      return "total_tokens";
  }
}

/**
 * A row's value for a column, and the maximum to scale its bar against.
 *
 * `null` means unreported — a provider that publishes no token accounting has
 * not burned zero tokens, and must not sort or draw as the quietest agent.
 */
export function columnValue(
  row: FleetRow,
  columnId: string,
  maxima: TelemetryFleetMaxima,
): { value: number | null; max: number } {
  switch (columnId) {
    case "tokens_per_hour":
      return { value: row.tokens_per_hour, max: maxima.tokens_per_hour };
    case "turns_per_hour":
      return { value: row.turns_per_hour, max: maxima.turns_per_hour };
    case "turns":
      return { value: row.turns, max: maxima.turns };
    case "files":
      return { value: row.files_touched, max: maxima.files_touched };
    case "lines":
      return { value: row.lines_added + row.lines_removed, max: maxima.lines };
    case "active":
      return { value: row.active_ms, max: maxima.active_ms };
    case "tokens":
      return { value: row.total_tokens, max: maxima.total_tokens };
    case "cpu":
      // A real ceiling, so this one is not fleet-relative.
      return { value: row.cpu_usage ?? null, max: 100 };
    case "memory":
      return { value: row.memory_mb ?? null, max: maxima.memory_mb ?? 0 };
    default:
      return { value: null, max: 0 };
  }
}

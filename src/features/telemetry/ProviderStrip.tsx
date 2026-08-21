import React from "react";

import { isUserFacingProviderName, providerDisplayName } from "../agents/providerOptions";
import { Sparkline } from "./Sparkline";
import {
  formatCount,
  formatDuration,
  GRAIN_LABELS,
  measureShortLabel,
  UNREPORTED,
} from "./telemetryFormat";
import type {
  FleetProviderRow,
  TelemetryFleetMaxima,
  TelemetryGrain,
  TelemetryMeasure,
} from "./telemetryTypes";

/** The habitat card's reserved provider name. */
const HABITAT = "all";

export interface ProviderStripProps {
  /** The habitat as a whole. Always the leading card. */
  habitat: FleetProviderRow;
  /** Providers in display order, habitat excluded. */
  providers: readonly FleetProviderRow[];
  /** Largest value across the provider cards, habitat excluded. */
  maxima: TelemetryFleetMaxima;
  /** What the sparklines carry, shared with the table's trend column. */
  trendMeasure: TelemetryMeasure;
  /** Bucket width, so a card can say what one sparkline column covers. */
  grain: TelemetryGrain;
}

/**
 * Where the habitat's work went, by provider.
 *
 * The table below answers "which agent is doing what". It cannot answer "which
 * provider am I actually running on", because that question is orthogonal to the
 * row model: fifty agents across four providers means aggregating fifty rows in
 * your head.
 *
 * Cards rather than more table rows. A provider has no status, no CPU, no
 * workspace, and is never "spinning", so the runaway detector the table exists
 * for has no meaning here — sharing the table would make every column mean two
 * things depending on which kind of row was being read.
 *
 * This is also the only stable element on the surface. Agent rows appear,
 * vanish, and re-sort constantly; provider cards keep their positions, which is
 * what makes the strip readable at a glance from the side of a screen.
 */
export const ProviderStrip: React.FC<ProviderStripProps> = ({
  habitat,
  providers,
  maxima,
  trendMeasure,
  grain,
}) => {
  const trendLabel = measureShortLabel(trendMeasure);
  const grainLabel = GRAIN_LABELS[grain] ?? null;

  return (
    // Horizontal scroll rather than wrapping or shrinking. A habitat can run any
    // number of providers, and both alternatives make the strip's height depend
    // on how many — which is the vendor-contingent layout this space was held
    // empty to avoid.
    // `flex-shrink-0` is load-bearing. The parent is a fixed-height flex column
    // whose other child is `flex-1` — basis `0%`, so its scaled shrink factor is
    // zero and it cannot give up space. That makes the strip the item that
    // absorbs a short viewport, and because `overflow-x: auto` forces
    // `overflow-y` to compute as `auto` too, the cards would quietly clip behind
    // an inner vertical scrollbar instead of the surface scrolling.
    <div
      className="dashboard-view__strip flex-shrink-0 flex gap-2 overflow-x-auto snap-x snap-proximity pb-1"
      role="group"
      aria-label="Activity by provider"
      tabIndex={0}
    >
      <ProviderCard
        card={habitat}
        // The habitat is the sum of the providers, so it dominates by
        // construction. Scaling it against them would peg it at full height and
        // scaling them against it would flatten every one onto the floor.
        // Comparable things share a scale; a total is not comparable to its
        // parts, so this one is scaled to itself and says so.
        sparkMax={habitat.spark.reduce((highest, value) => Math.max(highest, value), 0)}
        trendLabel={trendLabel}
        grainLabel={grainLabel}
        scaledToSelf
      />
      {providers.map((card) => (
        <ProviderCard
          key={card.provider}
          card={card}
          sparkMax={maxima.spark}
          trendLabel={trendLabel}
          grainLabel={grainLabel}
        />
      ))}
    </div>
  );
};

function ProviderCard({
  card,
  sparkMax,
  trendLabel,
  grainLabel,
  scaledToSelf = false,
}: {
  card: FleetProviderRow;
  sparkMax: number;
  trendLabel: string;
  grainLabel: string | null;
  scaledToSelf?: boolean;
}) {
  const name = cardName(card.provider);
  const trendTitle = [
    `${trendLabel} per ${grainLabel ?? "bucket"}`,
    scaledToSelf ? "scaled to this card, not to the providers below it" : null,
  ]
    .filter(Boolean)
    .join(" — ");

  return (
    <section
      className={`dashboard-view__strip-card flex-shrink-0 snap-start w-[168px] flex flex-col gap-1.5 rounded-xl border border-wardian-border/50 bg-[var(--color-wardian-card)] px-2.5 py-2 transition-opacity ${
        card.idle ? "opacity-40" : ""
      }`}
      aria-label={`${name}: activity over the window`}
    >
      <header className="flex items-baseline justify-between gap-1.5">
        <h3
          className="text-xs text-primary font-bold truncate"
          // The roster count is not a tile: it is the only figure on the card
          // that does not move with the window, and one window-independent
          // number among window-scoped ones is a trap. It belongs where someone
          // asking "why is this card here" will look.
          title={
            card.provider === HABITAT
              ? `Every provider this habitat has recorded — ${card.roster_agent_count} agents configured`
              : `${card.roster_agent_count} agents configured on ${name}, which is what places this card`
          }
        >
          {name}
        </h3>
        {/* Agents sits in the header rather than in the grid because it counts
            the *rows* the rest of the card is measured over, not another
            measure alongside them — and because five half-width tiles in a
            two-column grid leaves a hole. */}
        <span className="text-[10px] font-mono tabular-nums text-muted-neutral flex-shrink-0">
          {formatCount(card.active_agent_count)} active
        </span>
      </header>

      <dl className="grid grid-cols-2 gap-x-2 gap-y-1">
        <Tile label="Turns" value={formatCount(card.turns)} />
        <Tile
          label="Tokens"
          // Unreported is not zero. A provider without token accounting has not
          // been the cheapest one in the habitat, it has not been measured.
          value={card.total_tokens === null ? UNREPORTED : formatCount(card.total_tokens)}
          muted={card.total_tokens === null}
        />
        <Tile label="Active" value={formatDuration(card.active_ms)} />
        <Tile label="Files" value={formatCount(card.files_touched)} />
        {/* Full width. A half column fits about eight characters at this size,
            and a busy window renders "+123.4k/-98.7k" — which truncates to lose
            the removals entirely, turning a diverging figure into a wrong one. */}
        <Tile
          label="Lines"
          wide
          value={
            <>
              <span className="text-wardian-success">+{formatCount(card.lines_added)}</span>
              <span className="text-muted-neutral"> / </span>
              <span className="text-wardian-error">-{formatCount(card.lines_removed)}</span>
            </>
          }
        />
      </dl>

      <Sparkline values={card.spark} max={sparkMax} label={name} title={trendTitle} />
    </section>
  );
}

/** One figure with its name. No bar: six competing scales in one card is noise. */
function Tile({
  label,
  value,
  muted = false,
  wide = false,
}: {
  label: string;
  value: React.ReactNode;
  muted?: boolean;
  /** Span both columns, for a figure a half column would truncate. */
  wide?: boolean;
}) {
  return (
    <div className={`min-w-0 ${wide ? "col-span-2" : ""}`}>
      <dt className="label-small font-normal truncate">{label}</dt>
      <dd
        className={`text-[11px] font-mono tabular-nums truncate ${
          muted ? "text-muted-neutral" : "text-primary"
        }`}
      >
        {value}
      </dd>
    </div>
  );
}

/**
 * What a card is called.
 *
 * Known providers get their proper casing from the roster's own helper, so the
 * strip and the agent editor never disagree about whether it is "opencode" or
 * "OpenCode". An unknown provider prints as recorded — the facts may name one
 * the current build has never heard of, and inventing a label for it would be
 * worse than showing what is actually in the store.
 */
export function cardName(provider: string): string {
  if (provider === HABITAT) return "All";
  return isUserFacingProviderName(provider) ? providerDisplayName(provider) : provider;
}

# Dashboard Provider Activity Strip

- **Status:** Implemented
- **Date:** 2026-08-20
- **Fills:** the reserved top strip from [2026-08-14-dashboard-fleet-monitor.md](2026-08-14-dashboard-fleet-monitor.md), which allocated the space and deliberately left it empty.

## Context and Problem Statement

The Dashboard answers "which *agent* is doing what". It cannot answer "which
*provider* is this habitat actually running on", because that question is
orthogonal to the row model: an operator with fifty agents across four providers
reads fifty rows and has to aggregate in their head.

The fleet-monitor spec reserved a top strip for a cross-provider control and then
refused to build one, for a stated reason: only codex publishes a rate limit, so
a capacity gauge made the surface's shape depend on which vendor the habitat
happened to run. That objection is specific to **limits**. It does not apply to
**activity**, which every provider reports by construction — a provider with no
token accounting still has turns, active time, files, and lines.

So the strip gets the half that is universally answerable. Account limits stay
out of scope; see [Deferred](#deferred).

### Why a strip of cards and not more table rows

A provider is not an agent. It has no status, no CPU, no workspace, and it is
never "spinning" — the runaway detector the table exists for has no meaning at
provider granularity. Putting providers in the same table would force every
column to mean two different things depending on which kind of row you were
reading.

The strip is also the only element on this surface that is **stable**. Agent rows
appear, vanish, and re-sort constantly. Provider cards do not, which is what
makes them readable at a glance from the side of a screen.

## Proposed Decision

### Shape

A horizontally scrolling strip of fixed-width cards above the fleet table.

```
┌─ All      12 active ─┐┌─ Codex     7 active ─┐┌─ Claude    4 active ─┐
│ Turns      Tokens    ││ Turns      Tokens    ││ Turns      Tokens    │
│ 1.2k       160.2M    ││ 842        91.4M     ││ 310        54.0M     │
│ Active     Files     ││ Active     Files     ││ Active     Files     │
│ 8h 12m     318       ││ 5h 40m     201       ││ 2h 01m     88        │
│ Lines                ││ Lines                ││ Lines                │
│ +123.4k / -98.7k     ││ +54.1k / -12.9k      ││ +18.0k / -4.2k       │
│ ▁▂▅█▆▃▂▁▄█▅▂▁▂▄▃▁▂   ││ ▁▁▃█▇▄▂▁▂▆█▃▁▁▂▅▂▁   ││ ▁▄▂▁▁▅█▂▁▁▂▁▁▃▂▁▁▁   │
└──────────────────────┘└──────────────────────┘└──────────────────────┘
                  ◄──── horizontal scroll ────►
```

Two layout decisions the ASCII hides:

- **Agents sits in the header, not the grid.** It counts the rows the rest of
  the card is measured over rather than being another measure beside them — and
  five half-width tiles in a two-column grid leaves a hole.
- **The line delta spans the full width.** A half column fits about eight
  characters at this size, and a busy window renders `+123.4k/-98.7k`.
  Truncated, that reads as a figure with no removals: a diverging measure
  silently turned into a wrong one.

No mode switch and no second view. The strip does one thing.

### Card order

1. **`All` is always first.** It is the habitat, not a provider, and it is the
   card an operator lands on.
2. Then providers by **roster frequency**: how many configured agents name that
   provider, descending. Ties break on turns in the window, then on name.

Roster frequency rather than in-window activity, deliberately. Activity-based
ordering makes cards change position when the window setting changes, and a
strip whose left-to-right order depends on a control elsewhere on the surface
cannot be read from muscle memory. The cost is that a provider configured on many
agents but rarely run still ranks high; that is the correct answer to "how
frequently do I use each provider in my agents", which is a property of the
roster.

Providers present in the roster but silent in the window are still shown, after
the active ones and dimmed — the same rule the table applies to idle agents,
which it lists as available capacity rather than hiding.

"Silent" means **no recorded presence**, not "every figure rounds to zero".
`active_ms` is clamped to the window in whole seconds, so a sub-second span
rounds away while the agent that produced it is genuinely there; a card dimmed
on magnitude alone would contradict its own header, which reads "1 active". The
dimming predicate therefore includes the active-agent count.

### `All` is computed, not summed

The habitat card's agent count is a **distinct** count. Summing the provider
cards double-counts any agent that has run on two providers, and the number that
produces is not wrong-by-a-little, it is a different quantity. Every other tile
on the `All` card is additive and may be summed per bucket.

This is why `All` is a separate field in the DTO rather than the first element of
the provider array: a consumer that iterates `providers` must not be able to pick
up the habitat total by accident.

### Backend: extend `telemetry_fleet`, do not add a command

The strip shares the Dashboard's window setting and its trend measure. Reading it
from a second command would mean two reads against a moving trailing window, and
the strip could quote a window the table below it was not showing.

```rust
pub struct TelemetryFleetProviderDto {
    pub provider: String,
    /// Configured agents naming this provider. The ordering key. Window-independent.
    pub roster_agent_count: i64,
    /// Agents on this provider that recorded anything in the window.
    pub active_agent_count: i64,
    pub active_ms: i64,
    pub turns: i64,
    /// `None` when the provider publishes no token accounting. Never 0.
    pub total_tokens: Option<i64>,
    pub files_touched: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub tokens_reported: bool,
    /// Trend measure per bucket, aligned to `TelemetryFleetDto::buckets`.
    pub spark: Vec<i64>,
    /// Nothing recorded in the window. Still listed, dimmed.
    pub idle: bool,
}

pub struct TelemetryFleetDto {
    // ...existing fields unchanged...
    /// The habitat. `provider` is `"all"`; the agent count is distinct.
    pub habitat: TelemetryFleetProviderDto,
    /// One per provider in the roster, already ordered for display.
    pub providers: Vec<TelemetryFleetProviderDto>,
    /// Largest single value across the provider cards only — see below.
    pub provider_maxima: TelemetryFleetMaximaDto,
}
```

The TypeScript mirror in `src/features/telemetry/telemetryTypes.ts` uses the same
`snake_case` field names, per the IPC standard.

#### Totals come from `matrix_at`, not `breakdown`

`telemetry_dashboard` builds its provider rows from `breakdown`, which reads
`telemetry_rollup_hourly`. `telemetry_fleet` deliberately does not, and the
comment at `src-tauri/src/commands/telemetry.rs:519` records why: a trailing
window shorter than an hour begins *inside* a bucket whose start precedes it, so
`bucket_start >= from` matches nothing and every measured total collapses to
"unreported".

The strip must use the same source as the table beneath it. If the `codex` card
said 91.4M and the codex agent rows in the table summed to something else, the
surface is broken regardless of which figure is right. So the strip reads the
same fact tables at `Dimension::Provider`.

An alternative was considered and rejected: derive provider totals by folding the
already-computed per-agent grids through `AgentRow.provider`. It costs no extra
queries, and it misattributes history — the roster holds an agent's *current*
provider, while the facts hold the provider each turn actually ran on. Reading
the true mapping out of the facts does not rescue it either: an agent that used
two providers inside one window maps to both, and its cells cannot be split
after the fact. Over a window that reaches 90 days this is a real error, and a
silent one.

#### The cheap-because-few-rows assumption was wrong

This spec originally claimed the extra provider-dimension grids would be
negligible, on the theory that they return single-digit row counts against the
agent dimension's hundreds. **Measured, that is false.** The cost is the scan,
not the row count: on a real 1.2 GB store, a trailing 30 days touches 120,588 of
1,080,358 turn rows, and the query plan is an index range scan followed by a
table lookup for every one of them. Ten times fewer output rows bought a 10%
saving.

What is actually expensive is **building the time axis**. The strip prints six
figures and draws exactly one shape, so six of its seven grids were paying for
buckets that were computed and thrown away.

So `matrix_at` is called once, for the trend measure's cells. The six totals go
through a new `wardian_core::telemetry::matrix::totals_at`, which answers every
measure sharing a fact table in **one** `GROUP BY` and reuses `Measure::fact_expr`
so its aggregates cannot drift from the ones `matrix_at` uses. A test pins
`totals_at` against `matrix_at` across every dimension and every measure, because
an optimisation allowed to disagree with the thing it optimises is a bug
generator.

#### The table was making the same mistake, and this pays for the strip

`telemetry_fleet` built its per-agent totals by calling `matrix_at` six times
through a `totals_for` helper and discarding six sets of cells. That predates
this work; the strip only made it visible by repeating it.

Both halves now use `totals_at`. Measured on the same store, best of three:

| Window | Agent half | Provider half (new) | Whole read |
|---|---|---|---|
| 1 day | 12 → 5 ms | 10 → 5 ms | 22 → **10 ms** |
| 7 days | 168 → 73 ms | 148 → 65 ms | 316 → **139 ms** |
| 30 days | 1244 → 495 ms | 1239 → 482 ms | 2484 → **977 ms** |

The comparison that matters: before this change the Dashboard read the agent
half only, for 1244 ms at a 30-day window. It now reads the agent half *and* the
whole provider strip in 977 ms. **The surface gained an element and got faster.**

#### A covering index was measured and rejected

The remaining cost is the table lookup per scanned row. A covering index on
`telemetry_turns(ended_at, provider, session_id, turn_id, event_key,
input_tokens, output_tokens, cached_input_tokens)` removes it, and was built and
timed on a copy of the real store:

| | Before | After |
|---|---|---|
| Provider cells | 118 ms | 72 ms |
| Provider totals | 116 ms | 74 ms |
| Agent cells | 121 ms | 118 ms |

38% on two queries, nothing for the agent path, which uses
`idx_telemetry_turns_session_end` — and **160 MB** of index on a 1.28 GB store,
a 12% file growth for every user. Rejected. The index would also have to be
maintained on every ingest write, which is the hot path this store is actually
optimised for.

#### Coalescing concurrent reads, and why a TTL cache is not the fix

Telemetry surfaces are woken from two directions that can coincide: a backstop
interval, and the `telemetry-updated` event the **background ingest loop** emits
when a pass advanced a source. When those land within a few milliseconds, the
second issues a full read of a large store to answer a question the first is
already answering.

`useCoalescedRead` joins the second caller to the first read. This is exact
rather than approximate: nothing is answered from a stale result, because the
read being joined has not finished yet. Both `useFleet` and `useTelemetryMatrix`
use it.

**`refresh` deliberately bypasses it.** After an explicit ingest, a read already
in flight is one that queried the store *before* that ingest committed, so
joining it would render pre-refresh figures and leave them up until the next poll
— 15s on the Dashboard, 120s on Analytics. `refresh` calls the underlying read
directly. A test pins this by holding a poll's read open across the ingest and
asserting the surface settles on the post-ingest answer.

An earlier draft of this section claimed Refresh itself caused a double read, on
the theory that `telemetry_refresh` emits `telemetry-updated`. **It does not.**
`telemetry_refresh` calls `run_ingest_cycle`; the only emit site is the
background loop in `start_telemetry_ingest`. The duplicate reads are the
concurrent ones described above, and the staleness risk is one this change
introduced rather than one it found.

A TTL cache was considered and rejected. The Dashboard is a singleton surface on
a 15s poll against a *trailing* window, so any TTL short enough to keep that
window honest expires before the next poll and never hits — and it would have the
same post-ingest staleness problem, without an in-flight read to reason about.

#### The global connection is not reentrant

`get_db_conn` holds a plain `std::sync::Mutex` across its closure. Any helper
that takes the connection again from inside one deadlocks the thread, and
because that mutex guards the app's *single* global connection, everything else
touching the database wedges with it.

The roster lookup that orders the strip is exactly such a helper —
`roster_providers` → `get_all_agents` → `get_db_conn`. It is read before the
connection is taken, alongside `agent_labels`, which is hoisted for the same
reason.

This is not a rule the type system enforces, so it is pinned by a test that runs
the whole command against a real initialised store on a worker thread, **under a
timeout**. On timeout it exits the process rather than panicking: the wedged
thread still holds the process-global mutex and will never release it, so a
panic would report this one test and then hang every later one in the binary.
There is nothing to reclaim, so the run ends with a verdict.

The test was verified by reintroducing the bug: it hangs for the full timeout and
then fails, and passes once the read is hoisted back out.

### Normalisation: two scales, on purpose

Provider cards share one scale — `provider_maxima`, the largest cell across the
provider cards. The habitat card is normalised **against its own maximum** and is
the only card that is.

This looks like a violation of the surface's "the fleet is the denominator" rule
and is in fact that rule applied correctly. `All` is a different denomination
from one provider: it is the sum, so it dominates by construction, and putting
every card on the habitat's scale would flatten every provider sparkline onto the
floor — exactly the failure the square-root intensity function in `Sparkline`
already exists to prevent. Comparable things share a scale; the total is not
comparable to its parts.

The habitat card carries a `title` saying its trend is scaled to itself.

### Where the controls sit

The window control moves **above** the strip, where it used to sit between the
strip and the table.

A scope control placed between two things it governs reads as governing only the
lower one. Every figure on the strip comes from the same `telemetry_fleet` read
at the same `window_minutes` as the rows beneath it, but the old sequencing made
the strip look like it sat outside the window entirely — inviting the reader to
take its numbers as all-time.

This trades one scope error for a smaller one: **Columns** genuinely affects only
the table, and it now sits above the strip too. That is the cheaper confusion.
A reader who expects the strip's tiles to be configurable tests it once and
corrects; a reader who thinks the strip is not window-scoped silently misreads
every figure on it. The picker panel stays down beside the table, and the
button's tooltip names what it configures.

### Frontend

- New component `src/features/telemetry/ProviderStrip.tsx`, rendered in place of
  the `dashboard-view__reserved` div in `src/views/DashboardView.tsx`.
- Reuses the exported `Sparkline` from `DashboardView.tsx`. Tiles are label plus
  figure with **no bar** — a bar per tile would put six competing scales inside
  one card and the card would stop being glanceable.
- An unreported token total renders `UNREPORTED`, never `0`. A provider without
  token accounting has not spent nothing.
- Trend measure is `trendMeasureFor(prefs.sort.column_id)`, unchanged — the strip
  and the table's trend column always carry the same measure. No new preference,
  no new persisted state.
- Scrolling: `overflow-x: auto` with `scroll-snap-type: x proximity`. The
  container is `role="group"` with `aria-label="Activity by provider"`, is
  keyboard focusable, and each card is a `<section>` with an accessible name, so
  the strip is reachable without a horizontal wheel.
- The strip is `flex-shrink-0`, and that is load-bearing rather than defensive.
  Its sibling table is `flex-1`, whose basis of `0%` gives it a scaled shrink
  factor of zero, so it cannot yield — which makes the strip the item that
  absorbs a short viewport. Because `overflow-x: auto` forces `overflow-y` to
  compute as `auto` too, a shrunk strip would clip its cards behind an inner
  vertical scrollbar instead of the surface scrolling.
- Colour continues to mean state only. Every sparkline takes the accent colour.
- The strip is present whenever the table is, including the empty state, where
  every card reads zero. Its height does not change with content — that was the
  original objection to a vendor-shaped element, and it applies to this one too.

### Testing

| Layer | What it pins |
|---|---|
| `cargo test` | `All` agent count is distinct, not the sum of provider counts, when one agent has turns on two providers |
| `cargo test` | Ordering is roster-count descending after `All`, with a roster-only provider ranked above a busier one it outranks |
| `cargo test` | A provider with no token accounting yields `total_tokens: None` with `tokens_reported: false`, not `Some(0)` |
| `cargo test` | `totals_at` agrees with `matrix_at` for every dimension and every measure |
| `cargo test` | A measure matching no rows is answered as empty rather than absent |
| `npm run test` | `useCoalescedRead`: concurrent reads join, sequential reads do not, a changed question does not join, a rejection releases the slot |
| `npm run test` | Refresh settles on post-ingest figures with a poll's read held open across the ingest |
| `npm run test` | The window control precedes the strip in document order, and the column picker follows it |
| `cargo test` | The fleet read never takes the global DB mutex twice, under a timeout |
| Browser E2E | Strip order, unreported provider, dimmed silent provider, and real horizontal overflow at a narrow viewport |
| `npm run test` | `ProviderStrip.test.tsx`: `All` renders first; unreported tokens render the dash; idle providers render dimmed and last |
| Browser E2E | Strip renders and scrolls horizontally with a seeded multi-provider mock habitat |

Native E2E is not required: nothing here touches PTY, IPC beyond an existing
command, or the filesystem.

## Consequences

- **Positive**: Fills a space the previous spec reserved and left empty, without
  reintroducing the vendor-shaped layout that kept it empty.
- **Positive**: Answers "where is my capacity going" at the granularity a person
  actually buys capacity at, which is the provider, not the agent.
- **Positive**: No new command, no new preference, no new persisted state. The
  strip inherits the window and the trend measure already on the surface.
- **Positive**: Card order is stable under every control on the surface, so the
  strip can be read from position.
- **Positive**: The fleet read got *faster* despite gaining the strip — 2484 ms
  to 977 ms at a 30-day window on a 1.2 GB store, because `totals_at` also fixed
  the six discarded axes the agent half was already paying for.
- **Positive**: A poll and an ingest event landing together now cost one read
  rather than two, on this surface and Analytics both.
- **Negative**: `totals_at` is a second way to ask the store for a figure, so it
  can drift from `matrix_at`. A test pins the two together across every
  dimension and measure, which is the only thing keeping that honest.
- **Negative**: Roster-frequency ordering ranks a configured-but-unused provider
  above a busy one. Accepted as the literal meaning of the requirement, and the
  dimming rule keeps it from being misleading.
- **Negative**: Two normalisation scales in one strip. Justified above, and it
  needs the tooltip to be honest.
- **Negative**: A habitat on one provider gets a strip of two cards, one of which
  is the other's total. Acceptable — it still reads, and the layout does not
  change shape.

## Deferred

**Account limits per provider.** Cut from this piece of work.

The data is not there to support it honestly. Only codex publishes limits, and
`crates/wardian-core/src/telemetry/sources/codex.rs` parses only the `primary`
window — the logs also carry `secondary`, `credits`, `individual_limit` and
`spend_control_reached`, all currently discarded. Claude Code's JSONL carries no
rate-limit data at all, so a Claude gauge needs its OAuth usage endpoint, the way
CodexBar and Usage4Claude get theirs; that is a network dependency, a credential
path, and an undocumented endpoint, and it does not belong in a telemetry store
whose whole premise is reading local logs.

If it is picked up later, the order is: widen the codex parser first (free,
local, already-read bytes), then decide whether an authenticated provider poller
is a thing Wardian wants to own at all. It would be a sibling of the strip, not a
mode inside it.

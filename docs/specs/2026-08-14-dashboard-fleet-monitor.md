# Dashboard as Fleet Monitor

**Status:** accepted, in implementation
**Supersedes:** the Dashboard layout in [2026-08-13-habitat-telemetry-dashboard.md](2026-08-13-habitat-telemetry-dashboard.md). That spec's store, ingest, and Analytics sections still stand.

## The question this surface answers

A dashboard exists to make you **notice**. A report exists to let you **look up**. If nothing on the screen can change your next action, it is a report.

The Dashboard's model is a **process viewer**, not a usage report. The three things it is opened for, in the operator's own words:

1. Is any process going crazy?
2. Are resources hitting a bottleneck?
3. Where can I spend what is left, or reduce what I am spending?

It is left open on the side. It is not a morning digest, and it is not a diff of what changed — that is Inbox's job.

## Why the previous versions failed

Three iterations were built and rejected. Each failure was the same mistake in different clothes, and it was not layout.

| Attempt | Shape | Why it failed |
|---|---|---|
| 1 | Aggregate strip over per-measure panels | Duplicated Inbox; per-agent figures only for active time; rendered session UUIDs |
| 2 | Rows × time heatmap, one measure | One measure at a time; a row limit showed 4 agents of 54 |
| 3 | Per-agent table, many measures | Cumulative totals — a historical ranking, not a monitor |

The third is the instructive one, because the *unit was right*. A process viewer does show per-entity rows. What was wrong is that **every figure was a cumulative total over a horizon**. `htop` does not show total CPU-seconds since boot; it shows CPU% now, and the motion is the signal. "1d 8h active, 28.1M tokens over 7 days" is a ranking of history — true, and not actionable.

Three properties make a process viewer legible, and the totals table had none:

- **A bounded window, not all of history.** `htop` does not show total CPU-seconds since boot. A figure covering everything an agent has ever done cannot say whether it is misbehaving now.
- **Denominators.** `CPU%` sits against a known ceiling, so "going crazy" is visible without knowing what normal is.
- **State.** What a process *is doing* is a column. The totals table only reported what it had done.

The first of these was originally written as "rates, not totals", and that overshot — the fix was the window, not the division. See [The rates default was tried and reversed](#the-rates-default-was-tried-and-reversed).

## Boundary with Analytics

| | Dashboard | Analytics |
|---|---|---|
| Question | "Is anything wrong, and where is capacity going?" | "Exactly how much did X do between A and B?" |
| Time | One trailing **window**, a setting | Arbitrary **horizons**, a filter |
| Figures | Totals over the window, plus live state | Totals, read off an axis |
| Use | Left open | Opened to answer something |

The multi-horizon selector moves wholly to Analytics. A window *setting* and a horizon *filter* are different things, and shipping both on one surface is what made the Dashboard read as a report.

## Denominators without ceilings

Only codex publishes a rate limit, and live process metrics are off by default (below), so most columns have no absolute ceiling. That is fine, because on a fleet monitor **the fleet is the denominator**.

Every scaled visual — sparkline and bar alike — is normalized against the busiest row in the *table*, never against the row itself. Per-row normalization would draw an agent that ran ten minutes and one that ran all week as the same shape. Spotting a runaway does not need an absolute ceiling; it needs an outlier, and relative scale delivers exactly that.

## Mini-visuals

**A number alone is the exception, not the rule.** Every quantitative cell pairs its figure with an inline visual scaled to the fleet, in the manner of `htop`'s per-core bars:

| Column kind | Visual |
|---|---|
| State | Status dot, using the standard palette (Emerald idle, Cyan processing, Amber action required, Gray off, Red error) |
| Trend | Sparkline of the measure per bucket across the window |
| Rate | Value plus a bar scaled to the fleet maximum |
| Total | Value plus a bar scaled to the fleet maximum |
| Bounded (CPU, quota) | Value plus a bar scaled to its real ceiling |
| Lines ± | Diverging bar, added against removed |

Colour continues to mean **state only**. Bars and sparklines take the accent colour regardless of magnitude; a hot agent is tall, not red. Nothing on this surface is coloured to imply a judgment the data cannot support.

## Column model

Columns are user-selected, because Wardian is malleable software. That does not make the default arbitrary — **the default set is the product's opinion**, and most operators will never change it.

### Kinds

Every column declares what kind of quantity it carries, because the window setting otherwise breaks meaning silently:

- **identity** — name, class, workspace. No window.
- **instant** — live state with no window at all: status, CPU, memory, current model.
- **rate** — per unit time. *Unchanged* when the window changes.
- **total** — summed over the window. **Changes meaning** when the window changes, so its header names the window.

Without this, an operator can place "Tokens" and "Tokens/hr" side by side, see them disagree, and reasonably conclude the surface is broken.

### Defaults

`state · agent · trend · active · turns · tokens · files · lines`

Consumption against output is the runaway detector, and it needs no separate anomaly panel: an agent with the most tokens and no files touched is spinning, and sorting surfaces it the way a process viewer does. This keeps colour meaning state only, and avoids a hidden heuristic engine deciding what counts as "wrong".

### The rates default was tried and reversed

The default above is the *second* opinion. The first was `burn rate · throughput · output`, argued for at length in [Why the previous versions failed](#why-the-previous-versions-failed) above, and it was wrong in a way worth recording rather than quietly editing away.

The rate argument was right about **time** and wrong about **denomination**. A monitor showing cumulative totals since the habitat's first run is a historical ranking, and that critique stands — which is why the surface reads a bounded trailing *window* rather than all of history. But once the figures are windowed, dividing them by that window buys nothing and costs legibility. Over a day, real agent work collapses into `0.2/h` and `1.7/h`: arithmetically true, and impossible to hold or compare. "28 turns" and "1d 8h active" are quantities an operator already has intuitions about.

What actually made the rate version read better than the totals table it replaced was **the mini-visuals**, not the division. Those stayed; the division did not.

The `htop` analogy misled here. CPU% is a rate because the ceiling is a rate — a core is 100% or it is not, and there is no meaningful "total CPU" for a window. Tokens and turns have no such ceiling, so the fleet is the denominator instead (see [Denominators without ceilings](#denominators-without-ceilings)), and fleet-relative scaling works identically on a total.

### Available, off by default

Both rate columns survive as options, since the rate view is genuinely the right question for some readers:

- **Tokens/hr** — billable tokens (fresh input + output) per hour
- **Turns/hr** — turns per hour

They were named **Burn** and **Throughput**. Both named a judgment rather than a unit: a reader had to already know that burn meant tokens and throughput meant turns, and neither word says which. Columns on this surface are named for what they carry.

Their **ids** were renamed along with their labels, since nothing outside this file depends on them.

### The reversal could not reach anyone already running

The rename was first taken to be the migration too: preferences written in the rates era carry `visible: true` for `burn` and `throughput`, those ids stop resolving, and the merge falls through to the defaults. That fixed half the problem and hid the other half.

A real preferences file from this habitat:

```json
{ "id": "active", "visible": false },
{ "id": "tokens", "visible": false },
{ "id": "burn",   "visible": true  },
{ "id": "throughput", "visible": true }
```

`active` and `tokens` still exist, so their saved `false` still won. Dropping the renamed ids would have produced `trend · turns · files · lines` — rates gone, and the two columns the new default exists to show still missing.

The defect is in the merge rule, not in the naming. **Saved visibility wins for every id that still exists, so a change to the default set reaches only operators with no preferences file** — that is, nobody who was already running. The rule was written for *adding* a column, and it handles that case correctly; it simply cannot express "the product changed its mind".

So the defaults are now **versioned**. `DASHBOARD_PREFS_VERSION` is stamped into every saved file, and a file written against an earlier generation has its column choices and sort discarded in favour of the current defaults. The bump rule is narrow: **bump when a column's default visibility changes**, never merely when a column is added, because the default-anchored merge already covers additions and a needless bump throws away real user choices.

The **window survives** the reset. "Show me a week" says nothing about which columns belong on a Dashboard, and making someone re-pick it every time the column set is revised is friction for no gain.

This is a general hazard for any default-anchored preferences merge, the watchlist included: the property that makes it good at adding things makes it silently unable to change its mind.

Also off by default: **CPU** and **Memory**, per agent process tree. Real ceilings, and therefore real denominators — but per the operator, not usually a big deal, so they are opt-in.

### Idle agents

Not dead weight to hide at the bottom. On a resource monitor an idle agent is **available capacity**, which is directly the answer to "where can I spend what is left". They remain listed and grouped.

## Preferences and persistence

Two layers, following the existing watchlist convention exactly rather than inventing a parallel mechanism.

**Global prefs — the seed.** `DEFAULT_DASHBOARD_PREFS` plus `load_dashboard_prefs` / `save_dashboard_prefs`, written on **every change**. No save button, matching `persistWatchlistPrefs`; a failed write is non-critical and swallowed.

**Surface state — per instance.** The workbench persists and restores state per surface, and `DashboardSurfaceState` carries the same preferences so a Dashboard survives a reload as it was left.

**Found during implementation:** core view surfaces are `open_policy: "singleton"`, so there is only ever *one* Dashboard. Per-instance divergence therefore does not arise today, and the global prefs are the effective source of truth — the app holds them, mirrors them into surface state, and every change writes both. The state contract is implemented rather than deferred so that the day the Dashboard becomes multi-instance, each one already persists its own columns and window with no migration.

Nothing is ever explicitly saved, in either layer.

**A second finding, also from implementation.** Restoring Dashboard state now *tolerates* unknown keys and falls back to the defaults, matching `coerceAnalyticsState`. Previously it demanded a strictly empty object, so a surface carrying any stray key was treated as corrupt — and opening the Dashboard silently spawned a **duplicate** rather than reusing the singleton. Tolerant restore is the correct behavior for a surface whose state is a preferences blob that will grow.

**Merge is default-anchored, and versioned.** On load, columns are rebuilt as `DEFAULT.columns.map(def => saved.get(def.id) ?? def)` — the default list is authoritative for which columns exist and in what order; saved prefs supply only visibility. A column added in a later release therefore appears for existing users automatically, with no migration, and stale prefs cannot hide it. When `DASHBOARD_PREFS_VERSION` moves, saved visibility is discarded outright instead; see [The reversal could not reach anyone already running](#the-reversal-could-not-reach-anyone-already-running) for why anchoring alone was not enough.

**One list, two readers.** `DASHBOARD_COLUMNS` sets display order and `DEFAULT_DASHBOARD_PREFS.columns` sets the order the table draws in. They drifted, and the picker ended up offering columns in an order the table did not use, so ticking a checkbox produced a column somewhere else. A test now holds the two sequences identical.

**Known limit, accepted.** Once the Dashboard is multi-instance, the seed becomes whichever instance was touched most recently. That is what "last used" means. Pinning a canonical default would need a "set as default" action, which is the friction this model exists to avoid.

**Order is fixed, visibility is toggled.** Matching the watchlist, there is no drag-to-reorder. Column order matters far less on a monitor than column presence, and a parallel picker mechanism is harder to walk back than to add later.

## Window

A **setting**, defaulting to a trailing **24 hours**, offered from 15 minutes to 30 days and clamped at 90 days by the backend. The sparkline grain adapts to the window (`Grain::for_window_within`), so an hour yields 5-minute columns and a day yields 15-minute ones with nothing else changing; the grain in force is named in the trend column's tooltip rather than in a strip of prose above the table.

An earlier draft capped this at a day, on the theory that anything longer was Analytics' question. That was wrong for the same reason the rates default was: it hid an agent that worked hard on Tuesday. The boundary with Analytics is *arbitrary horizons and a readable axis*, not window length.

## Analytics interpretability

Three defects made the matrix harder to read than the activity artifact it
replaced, and all three were about the axis rather than the data.

**A single row of bucket labels.** At six-hourly grain across a week the axis
read `20 20 20 20` — naming the hour and hiding the only thing being asked, which
day. It is now two tiers: a date on every column that opens a local day, times at
a stride beneath, and the same day boundaries drawn through every row so a column
can be traced back without counting across from the axis.

**No scale.** The heat ramp is square-rooted on purpose, so ordinary cells stay
visible against a dominant one. That curve is unreadable without an anchor, so a
legend now names the busiest column in the measure's own units.

**A provider gauge in the corner.** Removed, for the reason it was removed from
the Dashboard: only codex publishes a limit, so its presence made the surface's
shape depend on which vendor the habitat ran. Account capacity belongs to the
Dashboard's provider element, which exists either way. Removing it also retired a
`telemetry_overview` round trip Analytics made on every load purely to feed it.

**Labels naming the field rather than the quantity.** "Fresh input" was invented
vocabulary: nothing on the surface said what made input fresh, and the answer —
that it was not served from the provider's cache — is the entire distinction
between it and the measure beside it. It is now **New input** against **Cached
input**, a pair whose names differ by exactly the property that separates them,
and their sum is **New input + output** rather than "Tokens (fresh + output)".
"Cache reads" became "Cached input" for the same reason, and "Lines changed"
became "Lines added + removed", since a rewritten line counts in both.

Each measure now also carries a one-line definition, reaching the reader by
tooltip on the selector and on the total column. Definitions do not go on the
page: a surface left open should not re-explain itself on every glance, which is
why the Dashboard's prose strip was removed. A compact form exists in parallel,
because the total column is eighty pixels wide and holds a word, not a
definition. A test requires every measure `Measure::parse` accepts to have all
three.

**No icon.** Analytics rendered with the generic window glyph in the tab strip,
because a core surface's icon token is simply its type and nothing required
`surfaceIcons.ts` to know about it — a miss that degrades silently rather than
failing. It now takes a run of offset bars against the Dashboard's gauge: a gauge
reads "now", offset bars read "over time", which is precisely how the two
surfaces divide. A test now fails when any registered surface falls through to
the fallback.

The activity artifact this is measured against does more still — real interval
bars rather than bucketed cells, night bands, a now-line — but those belong to a
Gantt of activity spans, where this grid stays general across dimension and
measure. Worth revisiting if activity remains the measure people actually open.

## Reserved space

The top strip is **allocated and left empty** for a forthcoming cross-provider usage and limits control — a fuller version of the codex rate-limit gauge. It is not built here. Consequently the provider summary shrinks to whatever still earns a place once that control lands, and rate-limit headroom is not duplicated on this surface in the meantime.

## Risks

| Risk | Response |
|---|---|
| Active time is inferred for three of four providers, so any utilization reading is soft | Never presented as a measurement; the other columns count turns and tokens, which are recorded rather than inferred |
| A short window makes a lumpy workload look empty or frantic depending on where it lands | Default window of a day; grain adapts; the sparkline shows the shape behind the number |
| Fleet-relative scaling means bars move when an unrelated agent spikes | Accepted: relative is the point. The figure beside the bar is absolute |
| An agent with no token accounting (antigravity) has no token figure at all | Rendered unreported, never zero, consistent with the store's nullable columns |
| Live state and stored telemetry come from two sources and can disagree at the edges | Instant columns are labelled as live; rate and total columns always come from the store |
| A single very large source can exceed the per-pass byte budget, because the budget is charged *after* a source is read rather than passed into it | Accepted. The budget bounds how many sources a pass starts, not how much one source reads; bounding a single read needs an in-source continuation cursor. A source is at most one provider log, and the newest are the smallest deltas |
| The opencode source is charged nothing against that budget | Accepted. Its reads are bounded by the agent's session set and its cursor, not by a file backlog |
| An archived turn recorded as `unknown` that is still growing can be frozen by the settle window | Residual. Only `pending_response` positively says "still running"; requiring a terminal status would discard the ~60% of archived turns recorded as `unknown` |

## Review findings and fixes

An independent review of the implementation raised eight findings. Seven were
real and are fixed; one was not reproducible.

| Finding | Outcome |
|---|---|
| `tokens_reported` derived from `breakdown`, which filters hourly rollups on `bucket_start >= from` — so a window shorter than an hour matched no bucket and flipped every measured token total to "unreported" | **Fixed.** Token presence now comes from `telemetry_turns` over the exact window |
| Workspace-discovered opencode sessions were claimed by *every* agent in that directory, filing the same turns under each | **Fixed.** Sessions are assigned to exactly one agent; recorded ids claim first, directory matches fill in only what is unclaimed |
| The shared opencode cursor could skip a session discovered *after* another session advanced it | **Fixed.** The source now fingerprints its session set, so a change invalidates the cursor and forces an idempotent re-read |
| Dashboard preferences were never written to or restored from surface state, despite the contract | **Fixed.** Restored state wins when a surface carries its own configuration; every change writes both it and the global seed |
| An archive turn awaiting its reply could be frozen by `INSERT OR IGNORE` after two quiet minutes | **Fixed.** `pending_response` turns are held back until they settle |
| `Grain::bucket_count` divided the span while `bucket_bounds` floors `from`, under-reporting unaligned windows by one and letting a capped axis exceed its cap | **Fixed.** The count is now derived from the bounds, so the two rules cannot drift |
| The byte budget is charged after an unbounded read, and opencode is charged nothing | Accepted as limitations; recorded in the risks above |
| "The matrix test module does not compile — duplicated function declaration" | **Not reproducible.** The function appears once and the suite compiles and passes |

## Deferred

- The cross-provider usage control (space reserved above).
- Column reordering.
- Any anomaly-scoring engine. Runaways are found by sorting on rates, not by a hidden model.

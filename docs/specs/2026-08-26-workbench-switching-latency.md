# Workbench Switching Latency: Profile and Remediation Plan

Date: 2026-08-26

## Purpose

Switching between workbench tabs and between agents is not smooth. This spec
records a measured profile of where that time goes, names the causes with
per-component evidence, and defines the remediation order.

The conclusion in one sentence: **a single tab click produces about eight full
React commits, and roughly half of that work is the workbench tab strip
re-deriving titles and badges for all twenty tabs on every one of those
commits.**

All four phases below are implemented on this branch. A tab switch costs 32%
less wall time and 55% less React work than `main`, and a telemetry tick no
longer re-renders the application at all. See
[Measured outcome](#measured-outcome).

## Method

Measurements come from `scripts/measure-workbench-performance.mjs` against the
54-agent / 20-tab / 4-group fixture defined by the
[full-surface performance audit](./2026-08-12-full-surface-performance-audit.md):
production Vite build, `react-dom/profiling`, isolated `WARDIAN_HOME`, timing
started at user input and stopped two animation frames after the tab reports
`aria-selected`.

Three instrumentation changes were needed to get attribution the existing
harness could not provide.

1. **Commit fan-out.** The harness recorded only `Math.max(...)` of the React
   commits inside an activation window. That reports one commit's duration and
   hides how many commits an activation caused. It now also records
   `tab_switch_react_commit_count` and `tab_switch_react_commit_total_ms`.
2. **Per-component attribution.** A diagnostic copy of the harness walks the
   committed fiber tree in `onCommitFiberRoot` and charges self time
   (`actualDuration` minus children) to the component that rendered. This is a
   throwaway diagnostic, not part of the committed harness.
3. **Harness repair.** The `dashboard selection` interaction targeted
   `.dashboard-agent-card`, markup the Dashboard stopped emitting when it became
   a fleet table, and the fixture never mocked `telemetry_fleet`, so the surface
   rendered its empty state regardless. `npm run perf:workbench` therefore threw
   before writing any result. The gate that exists to protect switching latency
   has not been completing. It now seeds fleet telemetry and exercises Dashboard
   column sorting.

Three caveats on reading the numbers. The measurement waits two animation frames
after the click, so roughly 32 ms at 60 Hz is measurement floor rather than
perceived lag. The attribution window spans the whole 20-switch loop, so its
per-switch self times run higher than the strict click-to-paint window — use the
attribution for **ratios between components** and the commit totals for absolute
cost.

And the harness is very sensitive to what else the machine is doing. An early
run of this profile was taken while unrelated builds and test suites were
running, and reported a 128 ms median tab switch and 53 ms of React work; the
same build measured on an idle machine reports 83 ms and 28.5 ms. Absolute
figures here are from idle runs except where a section says otherwise, and every
before/after pair was measured back to back from two worktrees under whatever
load was present at the time. Compare within a pair; never across runs.

## Measured profile

Branch `perf/tab-agent-switching` from `origin/main` @ `706eeb9c`, Windows,
production renderer.

| Measure | median | p95 |
|---|---:|---:|
| Tab switch | 83.1 ms | 113.0 ms |
| React commits per tab switch | **8** | 12 |
| React commit total per tab switch | **28.5 ms** | 50.1 ms |
| First surface activation | 79.9 ms | 146.7 ms |
| Group focus | 57.0 ms | 96.1 ms |
| Startup restore | 527.7 ms | 592.5 ms |
| Full-roster telemetry | 49.9 ms | 66.7 ms |
| Surface interaction | 69.1 ms | 140.3 ms |

The gate limit for tab switch p95 is 250 ms and the observed value passes it.
That gate is a regression limit, not a smoothness target: an 83 ms median is
five frames, and a 113 ms p95 is seven.

Tab switch by surface type (median):

| Surface | median |
|---|---:|
| graph | **113.0 ms** |
| garden | 112.4 ms |
| new-tab | 96.1 ms |
| agent-session | 91.0 ms |
| dashboard | 90.0 ms |
| agents-overview | 83.1 ms |
| inbox | 66.7 ms |
| files | 63.6 ms |
| workflows | 63.3 ms |
| library | 62.5 ms |
| browser | 47.3 ms |

The largest single commit is nearly flat across surface types (8.7–13.2 ms). The
variation between surfaces is not one expensive render; it is how much non-React
work the reveal does.

Two findings surfaced only because the harness now runs to completion:

- **Dashboard column sort is among the slowest measured interactions**, at
  140.3 ms median and 146 ms p95, alongside the new-tab command palette. It had
  no coverage at all before, because the Dashboard was rendering its empty state.
- **`full_roster_telemetry_p95_ms` reached its 100 ms gate under load**
  (99.7 ms and 118.3 ms). On an idle machine it sits at 66.7 ms, so the gate is
  not breached on `main` — but the margin is thin enough that a loaded CI runner
  can trip it.

The checked-in baseline at
`docs/research/workbench-navigation/workbench-performance-baseline.json` predates
both the Dashboard repair and the new commit-count measures, so it needs
regenerating as a separate reviewed step.

### Where the React work goes

Per-component render counts and self time, per tab switch, averaged over 20
switches. Reproduced across two independent runs.

| Component | renders / switch | self time / switch |
|---|---:|---:|
| `DockviewSurfaceTab` | **157** | **57.1 ms** |
| `DockviewSurfacePanel` | 143.9 | 17.0 ms |
| Dockview group header | 27.9 | 14.0 ms |
| Command/action palette | 2.5 | 2.2 ms |
| `AgentChatView` | 43.1 | 1.5 ms |
| Inbox item row | 75.2 | 1.4 ms |
| `AgentWatchlist` | 3.4 | 0.8 ms |
| React root commits | 7.4 | — |

There are 20 tabs. 157 tab-header renders per switch means **every tab header
re-renders on essentially every commit**, and there are about eight commits.

`AgentWatchlist` rendering 3.4 times per switch is effectively the count of
`App` renders, since it is a direct child of `App` with no memo boundary between
them. So of the roughly eight commits, about three originate in `App` and the
rest inside the workbench host and adapter.

## Root causes

### 1. The tab strip re-derives presentation metadata on every render

`DockviewSurfaceTab` (`src/layout/workbench/DockviewLayoutAdapter.tsx:623`)
calls, per tab per render:

- `runtime.surface_title(surface)`
- `runtime.surface_badges(surface)`
- `workbenchPaneTargets(runtime.document.root, groupId)`

`surface_badges`, and the non-agent branch of `surface_title`, both route to
`SurfaceRegistry.presentation()`
(`src/features/workbench/surfaceRegistry.ts:469`). For each call that does
`canonicalSurface` → `canonicalizeState` — **a full `validateWorkbenchDocument`
pass, a `JSON.stringify`, a freshly allocated `TextEncoder`, and a
`JSON.parse`** — then `restoreKnownSurface`, then two `deepFreeze` walks, then
`canonicalizeState` again for the badges. Nothing is cached, and the inputs are
frozen objects that only change when the document changes.

Measured directly (`vitest`, jsdom):

| Call | 20 surfaces, one pass | × 8 commits |
|---|---:|---:|
| `registry.presentation()` | 1.276 ms | 10.2 ms |
| `registry.resolve_surface()` | 0.982 ms | 7.9 ms |

`resolve_surface` is the panel-side equivalent, called from
`App.renderWorkbenchSurface` for every panel. Together that is roughly 18 ms per
tab switch recomputing metadata that is identical every time, and that is the
floor: non-agent tabs pay `presentation()` twice per render.

`workbenchPaneTargets` walks the layout tree and allocates a new array per tab
per render, which also denies `WorkbenchTab` any chance to memoize.

### 2. One context value invalidates every panel and every tab

`DockviewSurfacePanel` and `DockviewSurfaceTab` both consume
`useAdapterRuntime()`. That context value is a single `useMemo`
(`src/layout/workbench/DockviewLayoutAdapter.tsx:1161`) with seventeen
dependencies, including `render_surface` and `surface_title`.

`App` supplies both as **fresh closures on every render**:

- `renderWorkbenchSurface` is a plain function declared in the component body
  (`src/views/App.tsx:1465`), not a `useCallback`.
- `surface_title` is an inline arrow in JSX (`src/views/App.tsx:1970`).

So every `App` render invalidates the adapter runtime, which re-renders all 20
tab headers and every mounted panel. `DockviewSurfacePanel` additionally runs an
`Object.values(groups).find(...)` scan per panel per render to compute its own
visibility.

### 3. `App` re-renders about three times per tab switch, and owns everything

Fixed in phases 1 and 4. `App` no longer re-renders for telemetry at all.

`App` is the single orchestrator: 19 `useState` hooks, 23 store selectors, and
the entire tree — watchlist, sidebar, workbench host, every mounted surface —
constructed in one JSX expression. Anything that sets state in `App` re-renders
all of it. Three sources fire during a tab switch:

- **`useWorkbenchPersistence`** projects store status with
  `setHookStatus((current) => ({ ...current, conflict, save_error, is_dirty,
  save_pending }))` (`src/features/workbench/useWorkbenchPersistence.ts:272`).
  It returns a new object unconditionally, so **every** workbench store
  notification re-renders `App`. A tab switch produces at least three:
  `set_active_surface` sets `is_dirty`, the debounced save sets
  `save_pending: true`, and completion clears both.
- **`registry.sync_presentations`** runs in a layout effect keyed on
  `state.document.surfaces` identity. Every command replaces the document via
  `deepFreeze(structuredClone(document))`
  (`src/features/workbench/useWorkbenchStore.ts:221`), so that identity always
  changes, and the sync re-canonicalizes and re-validates every surface.
- Dockview's own group and panel state.

The clone-and-freeze itself is cheap — 0.07 ms for the fixture document. The
cost is that it guarantees maximal identity invalidation downstream.

### 4. Every `memo()` boundary in the app is defeated from above

There are 15 `memo()` wrappers in `src/`. The ones on surfaces —
`AgentsOverviewSurface`, `GraphSurface`, `GardenSurface`, `AgentTerminal` —
never hit, because `renderWorkbenchSurface` hands each of them a fresh set of
inline arrow props plus freshly normalized state objects on every `App` render
(`normalizeGraphSurfaceState(...)`, `normalizeAgentsOverviewSurfaceState(...)`).

`AgentSessionSurface` repeats the pattern one level down: it passes
`onPresentationStateChange={(a, b) => {...}}` as an inline arrow
(`src/features/workbench/surfaces/AgentSessionSurface.tsx:236`), which is enough
on its own to defeat `memo(AgentTerminal)` for every open agent tab.

### 5. Hidden surfaces render as often as visible ones (not worth fixing)

Measured and left alone — see
[Freezing hidden panels was tried and reverted](#freezing-hidden-panels-was-tried-and-reverted).

Agent sessions, browser, and files use `render_policy: "suspend_when_hidden"`,
which maps to Dockview's `"always"` renderer, so the panels stay mounted.
Nothing stops their React subtrees re-rendering while hidden. The evidence is in
the attribution: the Inbox surface's item rows render 75 times per tab switch
while the Inbox is not the active tab, and every mounted panel wrapper renders
about 7 times per switch.

### 6. Heavy surfaces pay their remount inside the activation commit

`SuspendedSurfaceRenderer`
(`src/features/workbench/surfaces/coreSurfaceDefinitions.tsx:66`) releases a
heavy renderer 250 ms after it is hidden and remounts it synchronously on
reveal. Graph (sigma/graphology) and Garden (konva) therefore rebuild inside the
frame that is supposed to show the tab. Graph switches at 212 ms; Agents
Overview, which mounts a grid of agent terminal slots, switches at 396 ms.

### 7. Streaming agents re-render the whole app per output line

`useAgentResourceController` handles `agent-json-event` — one event per JSON
line of provider output — with
`setCurrentThoughts((previous) => ({ ...previous, [session_id]: effect.thought }))`
(`src/features/agents/useAgentResourceController.ts:267`). There is no equality
bail-out, so an unchanged thought still allocates a new object and re-renders
`App` and everything under it. The same handler also writes the queue store
through `appendAgentEvent`. This is the mechanism behind lag that worsens while
agents are working, as distinct from switching lag.

`agent-metrics` (5 s) does the same: it rebuilds and sets the telemetry map
unconditionally, whether or not any value changed.

### 8. Startup loads everything eagerly

Only Monaco and PDF.js are code-split. The entry preloads roughly 3.4 MB of
minified JS, including `vendor-graph` (231 KB: sigma, graphology, xyflow),
qrcode, all four xterm chunks, and a 1.22 MB app chunk containing every surface,
the settings modal, the workflow builder, and the markdown pipeline. Startup
restore p95 is 1055 ms.

## Remediation plan

Ordered by measured gain per unit of risk. Each phase is verified by re-running
`npm run perf:workbench` against the same fixture.

### Phase 1 — Stop redundant renders (landed)

| Change | Expected effect |
|---|---|
| Bail out of `publishStoreStatus` when `conflict`, `save_error`, `is_dirty`, and `save_pending` are all unchanged | removes ~2 of ~3 `App` renders per switch |
| Bail out of `setCurrentThoughts` and the `agent-metrics` telemetry set when the value is unchanged | removes the per-output-line full-app render |
| `useCallback` for `App.renderWorkbenchSurface` and `surface_title` | stops the adapter runtime invalidating on unrelated `App` state |
| `useCallback` for `AgentSessionSurface.onPresentationStateChange` | restores `memo(AgentTerminal)` for every open agent tab |

Target: commits per tab switch from 8 to 4–5.

### Phase 2 — Make the tab strip cheap (landed)

| Change | Expected effect |
|---|---|
| Cache `SurfaceRegistry.presentation()` in a `WeakMap` keyed by the frozen surface object, invalidated on `presentation_version` | removes ~10 ms/switch; surfaces are frozen and replaced on change, so identity is a sound key |
| Same for `resolve_surface()` | removes ~8 ms/switch |
| Hoist `TextEncoder` out of `canonicalizeState`, and skip the `validateWorkbenchDocument` round trip when the input is already a frozen canonical surface | removes the dominant cost inside both |
| Split `AdapterRuntime` into a stable-callbacks context and a document context; memoize `WorkbenchTab` with stable per-surface callbacks and a memoized `workbenchPaneTargets(root, groupId)` | stops all 20 headers re-rendering per commit |

Target: `DockviewSurfaceTab` self time from 57 ms/switch to under 10 ms.

### Phase 3 — Be lazy (landed, with one part reverted)

| Change | Expected effect |
|---|---|
| ~~Freeze hidden panels while they are off screen~~ | **Reverted.** Measured worse: it moves the saved work into the reveal frame. See below. |
| Reveal in two passes: paint the surface frame and its chrome in the activation frame, mount the heavy renderer in a follow-up `requestAnimationFrame` or `startTransition` | Graph 212 ms and Agents Overview 396 ms collapse toward the ~77 ms floor; the tab responds in one frame and fills in |
| `React.lazy` the graph, garden, workflows, library, and settings surfaces | drops `vendor-graph`, konva, xyflow, qrcode and the markdown pipeline out of startup; improves restore p95 |

Deferring a reveal is a visible behaviour change, so each deferred surface needs
a skeleton that occupies its final geometry. A tab that switches instantly and
then reflows is not an improvement over one that switches in 200 ms.

### Phase 4 — Move surface data out of `App` (landed)

The structural fix behind causes 2 through 4. Surfaces should subscribe to the
roster, telemetry, and watchlist state they need rather than receiving it as
props threaded through `renderWorkbenchSurface`. Once each surface reads its own
slice, `App` re-rendering stops meaning "re-render the entire application", and
the existing `memo()` boundaries start working as written. This is a refactor
rather than a patch, and should follow phases 1 through 3 rather than block
them.

## Measured outcome

Both phases are implemented. Base and candidate were measured from two
worktrees on an idle machine with the same repaired harness, so the pair is
directly comparable.

| Measure | base | after | change |
|---|---:|---:|---:|
| React commit total per tab switch | 28.5 ms | **14.6 ms** | −49% |
| React commit total, p95 | 50.1 ms | **26.2 ms** | −48% |
| Tab switch, median | 83.1 ms | 64.0 ms | −23% |
| Tab switch, p95 | 113.0 ms | 100.7 ms | −11% |
| First surface activation, median | 79.9 ms | 64.0 ms | −20% |
| Group focus, median | 57.0 ms | 47.0 ms | −18% |
| Group focus, p95 | 96.1 ms | 64.5 ms | −33% |
| Startup restore, median | 527.7 ms | 461.0 ms | −13% |
| Full-roster telemetry, p95 | 66.7 ms | 51.9 ms | −22% |
| React commits per tab switch | 8 | 8 | unchanged |

The mechanism, from the same fiber attribution. Render counts are the load-
independent measure and the one to trust; the self times are indicative, since
the base attribution run was not as quiet as the base latency run.

| Component | renders/switch, base | renders/switch, after | self ms, base → after |
|---|---:|---:|---:|
| `DockviewSurfaceTab` | **157** | **56** | 57.1 → 4.5 |
| `DockviewSurfacePanel` | 144 | 63 | 17.0 → 2.2 |
| Dockview group header | 28 | 16 | 14.0 → 2.2 |

The tab strip no longer re-renders on unrelated app state. What remains is
Dockview re-rendering its own tab components when a group changes active panel,
which is roughly 2.8 renders per tab per switch and outside this adapter.

Bundle delta against the frozen base: 22,970 gzip bytes, well inside the
250 KB gate.

### Phases 1 to 3

Phase 3 was measured against the base commit a second time, back to back on a
machine that was busy with unrelated work. Absolute numbers are inflated
against the idle figures above — base tab switch reads 144 ms here versus 83 ms
idle — but both halves of the pair saw the same load, so the comparison holds.
This is the full phases 1 to 3 delta:

| Measure | base | phases 1–3 | change |
|---|---:|---:|---:|
| Tab switch, median | 144.2 ms | **81.3 ms** | −44% |
| Tab switch, p95 | 212.0 ms | **144.4 ms** | −32% |
| React commit total per switch | 55.1 ms | **22.2 ms** | −60% |
| First activation, median | 79.8 ms | 63.8 ms | −20% |
| First activation, p95 | 177.7 ms | **95.4 ms** | −46% |
| Group focus, median | 91.3 ms | 73.4 ms | −20% |
| Heavy surface resume | 147.4 ms | 114.8 ms | −22% |
| Full-roster telemetry | 66.5 ms | 50.5 ms | −24% |
| Surface interaction | 113.9 ms | 95.5 ms | −16% |
| Startup restore, p95 | 570.1 ms | 509.3 ms | −11% |

On the idle runs, first activation by surface tells the code-splitting story
most clearly: Graph 148 → 63 ms, Garden 128 → 97 ms, Workflows 63 → 44 ms,
New Tab 65 → 45 ms. Eager startup JavaScript went from roughly 3.4 MB to
2.9 MB minified, with Sigma (149 KB), Konva (195 KB) and xyflow (79 KB) now
fetched on first use.

### Phase 4, and the end-to-end result

Phase 4 moved the four churning projections — telemetry, app metrics, terminal
titles and provider thoughts — out of `useAgentResourceController` and into
`useAgentTelemetryStore`. Seven components now subscribe to just the slice
they display instead of receiving it threaded through `App`.

The whole branch against `main`, measured back to back:

| Measure | base | phases 1–4 | change |
|---|---:|---:|---:|
| React work per switch | 31.1 ms | **14.1 ms** | −55% |
| React work, p95 | 60.5 ms | **26.3 ms** | −57% |
| Tab switch, median | 95.2 ms | **64.7 ms** | −32% |
| Tab switch, p95 | 114.0 ms | 95.8 ms | −16% |
| First activation, median | 94.8 ms | 63.9 ms | −33% |
| First activation, p95 | 162.4 ms | **97.0 ms** | −40% |
| Group focus, median | 62.5 ms | 46.5 ms | −26% |
| Group focus, p95 | 95.0 ms | 65.8 ms | −31% |
| Full-roster telemetry | 50.0 ms | 33.4 ms | −33% |
| Heavy surface resume | 98.5 ms | 80.7 ms | −18% |
| Surface interaction | 75.4 ms | 66.2 ms | −12% |
| Startup restore, p95 | 992.5 ms | **548.5 ms** | −45% |

The guarantee phase 4 actually buys is not in that table, because the harness
has no metric for it: **a telemetry tick, an app-metrics tick, a thought and a
title change now cost the application zero renders.** Before, each of those
was a full render of every mounted surface, all twenty tab headers and the
54-row watchlist. A test pins it directly rather than leaving it to a
benchmark to notice.

One number moved the wrong way: **commits per tab switch went from 8 to 9.**
Store subscriptions schedule their own commits. Each is small and scoped to
the components that read the slice, and the total React work per switch
halved, so this is the trade working as intended rather than a regression —
but it is why the proposed commit-count gate stays loose.

### Freezing hidden panels was tried and reverted

The plan called for holding a hidden panel's subtree instead of re-rendering
it. It was implemented, measured, and removed.

It does less total work and it made the steady tab switch worse, because the
work it saves is work that was being spread over frames nobody was watching.
Thawing a frozen panel puts all of it into the frame the user is waiting on:

| Surface | phases 1–2 | with freezing |
|---|---:|---:|
| agents-overview | 69.3 ms | **297.3 ms** |
| graph | 114.3 ms | 161.6 ms |
| garden | 80.1 ms | 112.0 ms |
| agent-session | 64.0 ms | 86.7 ms |

Overall p95 went to 161.6 ms, worse than the 113 ms base. The lesson generalises:
for a surface that stays mounted, background re-rendering is a cache, and
dropping it trades a cost nobody notices for one they do. Phases 1 and 2 had
already removed most of the app renders that made hidden panels expensive, so
there was little left to win.

### What did not move

**The commit count is still 8 per tab switch.** Phase 1 targeted 4–5 and did
not reach it. The equality bail-outs removed the `App` renders they were aimed
at, but the remaining commits originate inside Dockview and the save queue
rather than in application state, so they need a different fix. They are now
cheap — the whole set costs less than a single commit used to — so the count is
no longer the thing to chase first.

**Graph is still the slowest surface to switch to** once its renderer has been
released, because the reveal still rebuilds a Sigma scene. Deferring that build
past first paint helped the cold case a great deal (148 → 63 ms) but the warm
case still pays for the rebuild itself. Raising the hidden grace period, or
keeping the scene and only detaching the WebGL context, is the next thing to
try there.

What remains is the prop threading in `renderWorkbenchSurface` itself. `App`
no longer re-renders for telemetry, so that closure churns far less often, but
it still hands every surface fresh inline callbacks whenever `App` does render.
Finishing the job means the surfaces taking their callbacks from a stable
navigation context too. That is a smaller, purely mechanical follow-up now
that the data half is done.
## Gate changes

The gates were set as regression limits against a 250 ms tab switch and could
not see render fan-out at all. With phases 1 and 2 landed, these are the limits
the current numbers support — each roughly 20% above the measured p95, which is
enough headroom for a loaded CI runner without going slack:

| Gate | Now | Proposed | Measured p95 after |
|---|---:|---:|---:|
| `tab_switch_p95_ms` | 250 | 125 | 100.7 ms |
| `tab_switch_react_commit_total_ms` (new) | — | 35 | 26.2 ms |
| `tab_switch_react_commit_count` (new) | — | 12 | 11 |
| `surface_first_activation_p95_ms` | 500 | 160 | 128.1 ms |
| `group_focus_p95_ms` | 175 | 85 | 64.5 ms |

The commit-total gate is the one that matters. A max-single-commit gate cannot
catch fan-out, which is exactly the regression this profile found: eight commits
reported as the duration of one. The count gate is deliberately loose at 12 —
the count did not improve and is not currently the lever, so it is there to stop
it growing, not to force it down.

These should be applied together with a regenerated checked-in baseline, as a
reviewed step separate from this branch.

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

Two caveats on reading the numbers. The measurement waits two animation frames
after the click, so roughly 32 ms at 60 Hz is measurement floor rather than
perceived lag. And the attribution window spans the whole 20-switch loop, so its
per-switch self times run higher than the strict click-to-paint window. Use the
attribution for **ratios between components** and the commit totals for absolute
cost.

## Measured profile

Branch `perf/tab-agent-switching` from `origin/main` @ `706eeb9c`, Windows,
production renderer.

| Measure | median | p95 | max |
|---|---:|---:|---:|
| Tab switch | 127.9 ms | 212.5 ms | 396.3 ms |
| React commits per tab switch | **8** | 12 | 12 |
| React commit total per tab switch | **53.4 ms** | 85.5 ms | 87.9 ms |
| Largest single commit per tab switch | 17.1 ms | — | 21.6 ms |
| First surface activation | 77.1 ms | 145.6 ms | 180.0 ms |
| Group focus | 78.6 ms | 142.8 ms | 147.9 ms |
| Startup restore | 516.7 ms | 1055.2 ms | 1055.2 ms |
| Full-roster telemetry | 66.6 ms | 99.7 ms | — |

The gate limit for tab switch p95 is 250 ms and the observed value passes it.
That gate is a regression limit, not a smoothness target: 128 ms median is about
eight frames.

Tab switch by surface type (median):

| Surface | median |
|---|---:|
| agents-overview | **396.3 ms** |
| graph | 212.5 ms |
| agent-session | 128.8 ms |
| garden | 128.4 ms |
| dashboard | 95.7 ms |
| workflows | 93.7 ms |
| inbox | 93.3 ms |
| files | 79.2 ms |
| library | 77.5 ms |
| new-tab | 77.4 ms |
| browser | 68.7 ms |

The largest single commit is nearly flat across surface types (14–22 ms). The
variation between surfaces is not one expensive render; it is how much non-React
work the reveal does.

Two findings surfaced only because the harness now runs to completion:

- **Dashboard column sort is the slowest measured interaction at 282 ms p95**,
  above every other surface interaction. It had no coverage before because the
  Dashboard was rendering its empty state.
- **`full_roster_telemetry_p95_ms` is at or over its 100 ms gate on `main`**
  (99.7 ms and 118.3 ms across two runs). A full-roster telemetry tick is
  supposed to be the cheap path.

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

### 5. Hidden surfaces render as often as visible ones

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

### Phase 1 — Stop redundant renders (low risk, no API change)

| Change | Expected effect |
|---|---|
| Bail out of `publishStoreStatus` when `conflict`, `save_error`, `is_dirty`, and `save_pending` are all unchanged | removes ~2 of ~3 `App` renders per switch |
| Bail out of `setCurrentThoughts` and the `agent-metrics` telemetry set when the value is unchanged | removes the per-output-line full-app render |
| `useCallback` for `App.renderWorkbenchSurface` and `surface_title` | stops the adapter runtime invalidating on unrelated `App` state |
| `useCallback` for `AgentSessionSurface.onPresentationStateChange` | restores `memo(AgentTerminal)` for every open agent tab |

Target: commits per tab switch from 8 to 4–5.

### Phase 2 — Make the tab strip cheap (the single largest win)

| Change | Expected effect |
|---|---|
| Cache `SurfaceRegistry.presentation()` in a `WeakMap` keyed by the frozen surface object, invalidated on `presentation_version` | removes ~10 ms/switch; surfaces are frozen and replaced on change, so identity is a sound key |
| Same for `resolve_surface()` | removes ~8 ms/switch |
| Hoist `TextEncoder` out of `canonicalizeState`, and skip the `validateWorkbenchDocument` round trip when the input is already a frozen canonical surface | removes the dominant cost inside both |
| Split `AdapterRuntime` into a stable-callbacks context and a document context; memoize `WorkbenchTab` with stable per-surface callbacks and a memoized `workbenchPaneTargets(root, groupId)` | stops all 20 headers re-rendering per commit |

Target: `DockviewSurfaceTab` self time from 57 ms/switch to under 10 ms.

### Phase 3 — Be lazy

| Change | Expected effect |
|---|---|
| Freeze hidden panels: when `visible === false` and the surface has already mounted, render a memoized subtree that bails out unconditionally until it becomes visible again | removes the hidden Inbox/session/browser render traffic |
| Reveal in two passes: paint the surface frame and its chrome in the activation frame, mount the heavy renderer in a follow-up `requestAnimationFrame` or `startTransition` | Graph 212 ms and Agents Overview 396 ms collapse toward the ~77 ms floor; the tab responds in one frame and fills in |
| `React.lazy` the graph, garden, workflows, library, and settings surfaces | drops `vendor-graph`, konva, xyflow, qrcode and the markdown pipeline out of startup; improves restore p95 |

Deferring a reveal is a visible behaviour change, so each deferred surface needs
a skeleton that occupies its final geometry. A tab that switches instantly and
then reflows is not an improvement over one that switches in 200 ms.

### Phase 4 — Move surface data out of `App`

The structural fix behind causes 2 through 4. Surfaces should subscribe to the
roster, telemetry, and watchlist state they need rather than receiving it as
props threaded through `renderWorkbenchSurface`. Once each surface reads its own
slice, `App` re-rendering stops meaning "re-render the entire application", and
the existing `memo()` boundaries start working as written. This is a refactor
rather than a patch, and should follow phases 1 through 3 rather than block
them.

## Gate changes

The current gates pass while the app is visibly laggy, and one of them has not
been running at all. Proposed after phases 1 through 3:

| Gate | Now | Proposed |
|---|---:|---:|
| `tab_switch_p95_ms` | 250 | 120 |
| `tab_switch_react_commit_count` (new) | — | 3 |
| `tab_switch_react_commit_total_ms` (new) | — | 25 |
| `surface_first_activation_p95_ms` | 500 | 250 |

The commit-count gate matters most. A max-single-commit gate cannot catch render
fan-out, which is exactly the regression this profile found: eight commits of
17 ms each, reported as "17 ms".

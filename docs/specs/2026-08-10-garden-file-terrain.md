# Garden File Terrain

- **Status:** Proposed
- **Date:** 2026-08-10

## Sources

- [Garden as a Metric Map](./2026-07-30-garden-metric-map.md) owns the position
  algebra, the stability contract, the districts, and the rule that decides what
  earns a unit. This document implements two of its deferred items — the
  aggregation tree with a level-of-detail budget, and parcels with real borders
  and drop targets.
- [Malleable Garden Design Philosophy](./2026-06-02-malleable-garden.md)
  establishes that Garden owns layout and annotation only, never the contents of
  the things it arranges.
- [Agent Change Review](./2026-08-01-agent-change-review.md) and
  [Agent Change Snapshots](./2026-08-02-agent-change-snapshots.md) own the change
  set, its baselines, and its attribution. This document paints that data; it
  does not recompute any of it.

## Context and Problem Statement

The Garden places agents and workflow blueprints. It does not show the thing
they act on. An agent's whole observable effect on the world is a set of file
writes, and the map that is supposed to be a habitat renders none of it — so
"what has this agent been doing to my disk" is answerable only in a sidebar
list, one agent at a time, with no spatial memory between looks.

Three pieces landed recently that make this tractable, and none of them existed
when the metric map was written:

- a file resource layer with verified renderers and a comparison lens
  (`features/files/`);
- change review with per-path `change_kind`, churn, `evidence`, `agent_ids`, and
  `turn_indices`, invalidated by the working-tree watcher rather than polled;
- per-turn content snapshots, so a baseline resolves to real content.

The naive design is to give every file a unit. That is wrong twice over, and
both refusals come from the existing spec rather than from taste.

## Two Constraints That Decide the Shape

**A file does not earn a unit.** The metric map's rule is that an entity which
is an *attribute* of another renders *on* it, and only an independent lifecycle
earns a position. Even granting a file its own lifecycle, the scale forbids it:
this repository alone has 1466 tracked files against 53 agents, so admitting
files as layout entities makes `n` in every superlinear stage a property of the
disk. Districts exist precisely to cap `n`. A file layer that uncaps it deletes
the reason districts were built.

**Change activity may not move anything.** The stability contract is enforced by
a type boundary: `LayoutInput` accepts no telemetry, so geometry cannot depend
on it. Churn, hotness, and recency are the same class of signal as status —
they change colour, tint, and emphasis, and nothing else. The intuitive design
where hot files drift toward the agent editing them is prohibited, correctly: a
map whose landmarks move when someone saves a file cannot be navigated, and its
stored positions stop meaning anything.

So files arrive as **terrain**, not as population. Terrain has extent and
subdivision; it does not have a position in the metric.

## Proposed Decision

Each district renders a **ground plane**: a deterministic treemap of the
directory subtrees its agents work in, drawn behind the units and clipped to the
district's own extent. Files and folders below the roots never enter
`LayoutInput`. Change data is painted onto that ground and never consulted while
building it.

```
ground(district) = clip( treemap( roots(district), frontier ), districtExtent )
paint(cell)      = f( changeSummary, now )     // separate pass, separate input
```

The separation is a type boundary, mirroring the one that already protects
geometry from telemetry: `TerrainInput` accepts no change data, and
`buildTerrainPaint` accepts no geometry it can alter.

The viewport is the one thing that crosses, and only as a scalar. `TerrainInput`
carries `minSubdivideArea` — the world-space area below which a cell is not
subdivided — derived from zoom. It can add or drop a whole level of detail; it
cannot alter a rect, because a cell's rect is a function of its parent's rect and
its siblings and of nothing else. `terrain.test.ts` asserts that a cell drawn at
two thresholds occupies the identical rect at both, which is the claim the whole
design rests on.

### Roots

A district's roots are the distinct normalized workspace paths of its agents:
`git_worktree_folder ?? folder`, through `normalizeEntityPath`. This is the same
value `resolveAgentDistrict` already partitions on, so ground membership and
district membership cannot disagree.

A district usually has one root. A team district spanning several workspaces has
several, and they become the top level of its treemap — which is the honest
rendering of a workstream that crosses repositories, and is exactly the overlap
the malleable-garden spec asked Garden to make visible instead of nesting away.

`get_explorer_root` is deliberately not used. It is one invoke per agent for a
value that only differs when an explorer override is set, and the district
partition already committed to the config path. One source, one answer.

### Area is a share of the parent, not a size

A treemap normally weights cells by a quantity. None is available: `FileNode`
carries `name`, `path`, `is_dir`, and `extension`, and a directory's recursive
size cannot be obtained without the crawl this whole design exists to avoid.

Weighting files by byte size while directories fall back to a constant would
produce a map where `package-lock.json` is the largest thing in a repository and
`src/` is a small square. That is not a defensible statement about a codebase.

Cells are therefore **squarified in a deterministic order** (directories first,
then name — the order `get_directory_tree` already returns), and a cell's area
is its share of its parent. Squarified rather than slice-and-dice because aspect
ratio is what makes a cell clickable and labellable at small sizes.

The consequence has to be stated because it reads as a bug otherwise, and was
reported as one: **a file can be drawn larger than a folder.** A loose file at a
repository root is a peer of `src/`, so it gets a peer's share; everything
inside `src/` then divides what `src/` received. Area encodes depth in the tree
and number of siblings. Anyone who has used a disk-usage treemap will read it as
size, so the legend says what it is.

The one weighting the data supports is `is_dir`: a folder holds at least one
thing and a file holds none, so folders take `DIR_WEIGHT` (3) shares to a file's
one. This is a constant claim, not an estimate of how much more. It is
admissible precisely because both values are known the moment the parent is
listed — a child's rect is fixed by its parent's single listing and cannot shift
as deeper listings arrive.

Weighting a folder by its actual subtree would be more informative and is not
available at any price the stability contract can pay: subtree size is only
known for the ingested frontier, so geometry would become a function of the
viewport and zooming in would resize everything already on screen. That is the
one thing this design may not do.

A consequence to state plainly: adding a file reflows its parent's cell
subdivision. That is a canonical record changing, which the stability contract
permits; it is bounded to one folder's children and cannot move a unit, because
units are not in the treemap.

### Ground is sized against the space the lattice actually reserved

`ringLattice.ts` sizes every ring against the districts it holds, using each
district's **unit extent** — the distance from its origin to its furthest unit.
Two constraints decide a radius and the larger wins, so neighbouring districts
clear each other's extents by `DISTRICT_MARGIN`.

The ground disc must live inside that reservation, and a fixed floor does not.
Two one-agent districts sit roughly 216 apart; inflating each to a 120 floor
gives two discs whose sum exceeds the gap, and they visibly bleed into each
other. `groundRadiusFor` therefore treats `MIN_GROUND_RADIUS` as a target rather
than a guarantee: a district is inflated towards it only as far as half the
distance to its nearest neighbour, less `GROUND_GAP`.

A district whose units genuinely reach further than that gap keeps its full
extent. At that point the units themselves overlap, and shrinking the ground
would hide a layout problem rather than fix one.

The radius is resolved once, in `computeGardenLayout`, and both the geometry and
the Konva clip read the resolved value. Re-deriving it in each consumer is how a
cell ends up wider than the clip that cuts it, which draws as a folder that
silently disappears at the district edge.

A consequence to state plainly: adding a file reflows its parent's cell
subdivision. That is a canonical record changing, which the stability contract
permits; it is bounded to one folder's children and cannot move a unit, because
units are not in the treemap.

### Level of detail is the ingestion boundary

`facets.ts` already commits to this: files are not corpus members until a folder
is expanded, expansion increments `df` along the ancestor chain only, and the
drift penalty absorbs the perturbation. This document makes the same boundary
govern drawing.

A folder is **listed** — one `get_directory_tree` call — only when its cell's
screen-space area exceeds `EXPAND_AREA_PX`. The set of listed folders is the
**frontier**. There is no recursive enumeration anywhere in the design, and no
call is made for a folder the user cannot see.

Two budgets bound the result:

| Budget | Limit | What it protects |
| --- | --- | --- |
| `MAX_TERRAIN_CELLS` | 2000 drawn cells | Konva scene-graph size and hit-testing |
| `MAX_FRONTIER_DIRS` | 400 listed folders | Cached listings, and calls in flight |

When a budget binds, expansion stops at the *shallowest* level that fits, so the
map degrades by showing coarser territory rather than by showing an arbitrary
subset of a level. A folder whose children were not listed draws as solid ground
with a chevron, which is a legible "there is more here" rather than a lie about
emptiness.

**The cell budget is divided between districts, not shared.** This is a
stability requirement, and it was learned the hard way: with a single pool, the
deepest drawn level is a function of the *total* cell count, so a listing
arriving in one district silently deletes a level in a district on the other
side of the map, and the next invalidation there puts it back. The map blinks in
places nothing happened to, which is indistinguishable from a rendering fault
and destroys any trust in the ground as a stable surface. Each district
therefore descends against `districtCellBudget(maxCells, districtCount)` —
`maxCells / districts`, floored at `MIN_DISTRICT_CELLS`. A district's detail
must depend on that district's data.

**The floor is sized against what one level costs, and this was got wrong
once.** The cut is whole-level, so a share too small to admit a level does not
show less detail — it shows none, and the ground draws as bare squares that
never open however far the user zooms. A real district holding four repository
roots of ~46 entries each needs ~190 cells for its *first* level; a floor of 64
gave exactly the bare-squares failure, on a 37-district roster where
`2000 / 37 = 54`. The floor is now 512.

Being generous is safe because the budget is not what bounds the map. **The
frontier is.** A folder is listed only when its cell is large on screen, so
districts the user is not looking at contribute one cell each regardless of what
they are permitted, and the nominal ceiling of `districts × 512` is never
approached in practice. The budget is a backstop against one pathological folder
with thousands of entries, not the mechanism that keeps the scene graph small.
Reading it as the latter is what produced the 64.

Listings are cached by normalized path and **refreshed** — never evicted — when
`explorer-changed` fires, using the same 150 ms-debounced watcher the Changes
pane uses, which already excludes `.git`, `node_modules`, `target`, `.venv`,
`dist`, `build`, `.next`, `.turbo`, and `.cache`. The terrain inherits that
filtering and does not reimplement it.

The refresh rule replaces an earlier evict-by-prefix rule that dropped every
listing beneath the changed root. That was wrong in the way that matters: it
collapsed the district to a bare square for a debounce plus a round trip, and an
agent writing steadily kept it collapsed — the terrain visibly disappearing and
reappearing while work was happening, which is precisely when it is worth
looking at. Refreshing costs one stale render instead, and a directory deleted a
moment ago survives until its parent's listing lands. That is a far smaller lie
than a district that keeps vanishing.

Scoping is by **parent**, because a directory listing is what asserts a child
exists: refreshing the parent of a changed path adds new children and removes
deleted ones in the same call, and a deleted directory's own stale listing is
orphaned rather than drawn. This relies on `changed_paths`, which accumulates
into a `BTreeSet` across the debounce window rather than sampling it, so the set
is complete for that window — the original objection to using it does not hold.
An empty set falls back to refreshing every cached listing under the root. A
refresh that fails is the one case where a listing *is* dropped: the directory is
gone or unreadable, and keeping its children on screen would be the map
asserting what it just failed to confirm.

**Nothing polls.** `ExplorerPanel`'s three-second `git_status` interval is
explicitly not copied; the change-review spec already forbids it for exactly the
per-render cost it would impose here at far greater scale.

### Viewport drives the frontier, and only the frontier

The frontier depends on zoom, which means a viewport change can add cells. It
can never move one: cell geometry is a function of the frontier and the district
extent, both of which are computed in world space. Zooming in subdivides a cell
into its children; it does not resize the cell.

Viewport updates are coalesced to one frontier evaluation per animation frame,
and expansion requests are debounced at 200 ms so a wheel gesture that sweeps
through four zoom levels issues listings for the level it lands on, not for each
level it passed through.

### Change is paint

`ChangeReviewSummary` supplies everything the paint needs. For each root, one
`load_change_review` call yields per-path `change_kind`, `insertions` and
`deletions`, `evidence`, `agent_ids`, `turn_indices`, and `reviewed`.

**One call per root, not per agent.** `read_turns_for_workspace` in
`change_review.rs` selects conversations by workspace, so attribution already
spans every agent that worked there; the request's `agent_id` selects only the
watermark and the snapshot baseline. A 37-district install therefore costs a
handful of `git status` invocations rather than one per agent.

### Entry paths are rooted at the repository, not at the agent

Git reports every path relative to the **repository root**, whatever directory
the command ran in — verified by running the same `git diff --numstat` from a
subdirectory and getting identical output. An agent's folder is not required to
be a repository root, so joining an entry's path onto it produces a
real-looking absolute path that resolves to nothing whenever the agent is
scoped below the root. Nothing errors: the paint is computed, keyed to paths no
cell will ever carry, and the ground is silently blank.

`load_change_review` therefore returns `workspace_root`, resolved once with
`git rev-parse --show-toplevel` on a call that already runs several git
commands, and both surfaces join against that. It is `null` for a workspace
with no repository, where the paths came from turn records and *are* relative
to the requested directory. For a worktree it is the worktree's own root, which
is where the reported paths live.

The requested root and the path root stay distinct rather than one overwriting
the other. The requested root is what `roots` names and what the change-set
cache is keyed by; the watermark also keys on it, because "what has this agent
seen" is a question about the agent rather than about the repository.

### Which baseline the ground uses

Three of the five baselines — `last_effective_turn`, `conversation_start`, and
`unreviewed` — resolve against one agent's snapshots and watermark. That is
exactly right for the Changes pane, which shows one selected agent. It is
meaningless for a map showing every agent at once: picking a representative
agent per root would make the entire colouring depend on an arbitrary choice
that nothing in the UI could explain.

`head` and `branch_point` are computed from the repository and name no agent. So
the terrain adopts the pane's stored preference when it is one of these, and
falls back when it is not. This is a deliberate, narrow divergence from the "one
definition of changed" rule, and it is made safe by stating the baseline in
words in the legend: the two surfaces may differ, and they may never differ
silently.

**The fallback is `branch_point`, and which one it is matters more than it
looks.** The stored preference defaults to `last_effective_turn`, which is
agent-scoped — so every install that has never changed the setting lands on the
fallback rather than on a chosen value. An earlier `head` fallback therefore
made the out-of-the-box ground show only uncommitted work: blank across a tidy
roster, and blank for precisely the repositories whose agents had finished and
committed, which are the ones worth looking at. At map scale the question is
"what has this branch done", and that is `branch_point`.

Paths roll **up** the tree: a folder cell carries the aggregate churn of its
subtree whether or not that subtree is listed. This is the property that makes
the map readable at map scale — you should not have to expand a folder to learn
that something happened inside it — and it is why the rollup is computed from
the change set's paths rather than from the drawn cells.

| Channel | Encodes |
| --- | --- |
| Hue | `change_kind`: added, modified, deleted, renamed, untracked, or `mixed` |
| Opacity | Churn and recency together, `0.6 · churn + 0.4 · recency`, mapped onto `[0.12, 0.55]` |
| Stroke dash | `evidence`: `attributed` solid, `inferred` dashed |
| Dimming ×0.4 | `reviewed` — every changed path beneath the cell has been opened |
| Dimming ×0.3 | The cell's own children are drawn, so they are already saying this |

Churn is `log1p(insertions + deletions)` normalized against the **largest cell
on the map**, not within each root. Per-root normalization would give a
repository with three changed lines the same saturation as one with three
thousand, which is precisely the comparison a map showing several repositories
exists to make. Recency is `2^(-turnsAgo / 8)`, in turns rather than wall clock
because turns are what the change set carries and because an agent that ran
overnight and one that ran a minute ago produced the same amount of work; an
`inferred` entry carries no turn index and paints at a flat 0.55, since zero
would hide exactly the writes `inferred` exists to surface and full strength
would claim a freshness the data does not support.

`mixed` is a real answer rather than a fallback. A folder holding an addition
and a deletion is not "modified", picking the most frequent kind would let one
more file flip a folder's colour, and a precedence order would assert that
deletions matter more than additions — a claim about the work, not about the
data.

**Tints composite, so only the finest statement paints at full strength.** A
folder and its child are two rects in the same place; painting both at their
computed alpha stacks them (two washes at 0.5 read as 0.75, three as 0.87) and
the map ends up encoding *nesting depth* in the channel reserved for churn. A
cell therefore paints fully only where it is the finest thing saying it — a
file, or a folder whose contents are not drawn. A folder whose children are on
screen keeps `EXPANDED_TINT_SHARE` (0.3) of its tint and lets them carry the
signal. Without this a collapsed root reads as one flat orange disc, which is
both unreadable and, since every repository has changes, uninformative.

A deleted path has no cell of its own — it is not on disk — so it paints its
parent folder and is listed in the selection summary. Inventing a cell for a
file that does not exist would put a hole in the treemap that no listing can
reproduce, and the next `explorer-changed` would silently close it.

Change fetching is bounded the same way expansion is: only roots whose ground is
actually drawn, at most `MAX_CHANGE_CONCURRENCY` (4) in flight, cached per root
with the `explorer-changed` invalidation already established. On a 37-district
install this is a handful of `git status` invocations on load, not one per agent.

### Attribution threads are the one relation that earns a line

The metric map's deferred edge policy says only *flow* relations should draw
lines, because structural and affiliation relations are already geometry. An
`attributed` change is a flow: this agent wrote this path at this turn. It is
the first relation in the Garden that qualifies.

Threads are drawn only for the **selected** agent or the selected cell, capped
at `MAX_THREADS` (24) by descending churn. `inferred` entries — writes performed
through a shell, which the change-review spec is explicit are a lower bound and
never a filter — get tint and a dashed border, and no thread. The visual
difference *is* the evidence discriminant, rather than a badge in a tooltip.

### Selection answers with a set

A cell has no unit, so "who touched this?" is answered by highlighting the
agents in `agent_ids` — the same shape as `agentsCarrying` for skills, and for
the same reason: a set is the honest answer, a point never was.

The inverse is the higher-value direction and is the reason to build this:
selecting an agent tints every patch of ground it has written to, across every
district. No list view can show that.

### Opening reuses the file surface

Single click selects. Double click routes through `openFileWithSettings`, so the
user's per-family file-open preferences hold, and — when the path is in the
change set — opens the comparison lens against the active baseline, exactly as
the Changes pane does. Garden becomes a spatial entry point to the viewer that
already exists. It does not gain a viewer.

Opening marks the path reviewed through `save_change_review_watermark`, because
the change-review spec defines opening as the act of reviewing. A second surface
that showed diffs without advancing the watermark would make `unreviewed` mean
different things depending on where the operator happened to click.

## What Is Not Built

**Files as layout entities.** Rejected above, twice.

**Any path from change data into geometry.** `TerrainInput` accepts no change
data. This is a compile-time guarantee, not a convention.

**A second file tree or file viewer.** The Explorer pane and the files surface
already exist.

**Recursive enumeration, at any point, for any reason** — including to compute
directory weights, `df`, or a rollup. The rollup walks the change set's paths,
which is bounded by the number of changed files.

**Folders as placed units.** A folder could defensibly earn a position in the
metric, but that would put folders and agents in the same overlap-removal pass
and make district extent a function of how many directories a repository has.
Terrain is a subdivision of territory the district already occupies, which costs
the layout nothing.

**Terrain in the persisted scene.** The frontier is derived from the viewport and
the disk; persisting it would store a value that the next `explorer-changed` may
contradict. Expansion state is session-scoped.

**Drag and drop onto ground.** The parcels-as-drop-targets item stays deferred:
a drop would have to mean something canonical (move the file? re-anchor the
agent?), and neither is decided.

## Data Model

`snake_case` at every IPC boundary; internal TypeScript follows the Garden's
existing `camelCase` for pure modules.

```ts
/** A listed directory. The unit of ingestion. */
interface TerrainListing {
  path: string;              // normalized, absolute
  children: TerrainChild[];  // directories first, then name
  listed_at: number;
}

interface TerrainChild {
  name: string;
  path: string;
  is_dir: boolean;
  extension: string | null;
}

/** Geometry input. Accepts no change data and no telemetry, by construction. */
interface TerrainInput {
  districts: ReadonlyMap<string, TerrainDistrict>;  // districtId -> roots + extent
  listings: ReadonlyMap<string, TerrainListing>;    // the frontier
  /** World-space area below which a cell is not subdivided. Derived from zoom. */
  minSubdivideArea: number;
  maxCells: number;
}

interface TerrainDistrict {
  roots: readonly string[];
  origin: GardenPosition;
  radius: number;
}

/** Output. `rect` is world space; `depth` drives label and stroke weight. */
interface TerrainCell {
  path: string;
  name: string;
  isDir: boolean;
  districtId: string;
  depth: number;
  rect: { x: number; y: number; width: number; height: number };
  /** True when the cell has children that the budget or frontier excluded. */
  truncated: boolean;
}

/** Paint. Derived per render from the change set; never an input to geometry. */
interface TerrainPaint {
  kind: ChangeReviewChangeKind | "none";
  churn: number;          // 0..1, normalized per root
  recency: number;        // 0..1, decayed
  evidence: ChangeReviewEvidence | null;
  reviewed: boolean;
  agentIds: readonly string[];
}
```

## Budgets

| Constraint | Limit |
| --- | --- |
| Drawn cells | 2000 |
| Listed folders | 400 |
| `get_directory_tree` calls per frontier evaluation | 32 |
| Change-review calls in flight | 4 |
| Frontier evaluations | ≤ 1 per animation frame |
| Expansion debounce | 200 ms |
| Terrain work on the layout's critical path | none |
| Polling of any kind | none |

Terrain never blocks a layout pass and never participates in one. A terrain
failure — an unreadable directory, a change-review error — degrades that root to
undrawn ground and is never converted into a Garden failure.

## Implementation Plan

Ordered so each slice ships standalone and is verifiable on its own.

**Slice 1 — Ground geometry.** `terrain.ts` (squarified treemap, pure),
`terrainFrontier.ts` (LOD rule and budgets, pure), `useGardenTerrain.ts`
(listing cache, `explorer-changed` invalidation, debounced expansion),
`TerrainLayer.tsx` (Konva ground beneath the units, clipped per district). No
change data, no interaction. Districts publish origin and extent from
`computeGardenLayout`, which currently keeps them per unit.

**Slice 2 — Change paint.** `terrainPaint.ts` (rollup and channel mapping,
pure), `useTerrainChanges.ts` (per-root `load_change_review`, shared baseline
prefs, concurrency cap). Legend gains the change channels.

**Slice 3 — Interaction.** Cell selection and the summary bar, double-click
open through `openFileWithSettings` with baseline comparison and watermark
advance, attribution threads, and the agent-to-ground highlight in both
directions.

The watermark raised a question the design had not: a watermark is keyed by
agent and workspace, and the map has no selected agent. It advances for **every
agent in the path's `agent_ids`** — those are exactly the agents whose work was
just read, and unlike a representative-agent choice it is not arbitrary. A path
no agent claimed advances nothing, which is the same refusal that keeps an
`inferred` write from being threaded to anybody.

**Slice 4 — The agent's private plot. Measured, and not built.**
`~/.wardian/agents/<id>/workspace` was to render as a small plot on the agent
unit, answering "did it write to my repo or to its own scratch space". The
measurement says no.

On the reference install, **9 of 139 agent directories have a `workspace/` at
all**, and in 8 of those 9 its entire contents are one `temp/` directory. The
ninth adds `tools/`. Nothing in Wardian creates these; they exist only where an
agent followed an instruction to put scratch files there.

Building it as specified would add a root to the districts of 6.5% of agents
and — because cells are equal-weight — halve the ground area of the repository
those agents actually work in, to display an empty folder. The slice inverts its
own emphasis at the moment it succeeds.

There is also no cheaper honest version. A private workspace is not a git
repository, so it carries no change paint; and a write inside one never reaches
the change set, because `load_change_review` is scoped to a workspace root and
the private directory is outside every root. The map is silent about scratch
space, and at these numbers silence is the accurate rendering.

Worth revisiting only if agent scratch usage becomes common, which is a product
change rather than a rendering one.

## Verification

- Terrain geometry is identical for identical `TerrainInput`, across runs.
- `TerrainInput` cannot express change data or telemetry; a change to the change
  set produces byte-identical cell rects.
- No unit position changes when a file is added, removed, or edited.
- Zooming subdivides cells and never resizes or moves an existing cell.
- A frontier evaluation issues at most 32 `get_directory_tree` calls, and none
  for a folder outside the viewport.
- A wheel gesture crossing four zoom levels issues listings for one level.
- When the budget binds, the deepest level is dropped whole rather than
  partially, and only within the district whose data bound it: a district's
  drawn depth is unchanged by what any other district ingests.
- A folder with unlisted children draws as truncated ground, not as empty.
- `explorer-changed` for a root refreshes only the cached listings that name a
  changed path, and no cell disappears while the replacement is in flight.
- A cell's change tint does not grow with its depth in the tree.
- No two districts' ground discs overlap, at any roster size.
- A folder is drawn larger than a file it sits beside.
- No `git_status` or `get_directory_tree` call is issued on a render that
  changed only telemetry, selection, or zoom within one detail level.
- A changed path inside an unlisted folder still tints its nearest drawn
  ancestor.
- A deleted path paints its parent and appears in the selection summary, and no
  cell is invented for it.
- An `inferred` entry draws dashed with no thread; an `attributed` entry draws
  solid and threads to its agent when selected.
- Selecting an agent highlights every cell it wrote across all districts.
- Selecting a cell highlights exactly the agents in its `agent_ids`.
- Double-clicking a changed file opens the files surface with the comparison
  against the pane's baseline, and advances the watermark for that path.
- An unreadable directory degrades that subtree to undrawn ground and leaves the
  rest of the map working.
- A non-git workspace renders terrain with no paint rather than an error.

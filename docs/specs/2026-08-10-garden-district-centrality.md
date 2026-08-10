# Garden District Centrality

- **Status:** Proposed
- **Date:** 2026-08-10

## Sources

- [Garden as a Metric Map](./2026-07-30-garden-metric-map.md) owns the position
  algebra, the stability contract, the ring lattice, and the rule that districts
  are sticky. This document adds one input to slot assignment and retunes ring
  radii; it changes nothing about `pos(e) = pin(district(embed(e) + local(e)))`.
- [Garden File Terrain](./2026-08-10-garden-file-terrain.md) owns the ground, its
  radius, and the change paint drawn on it. This document makes the lattice
  reserve the room the ground asks for, rather than leaving the ground to fit
  into whatever the lattice happened to leave.
- [Agent Change Review](./2026-08-01-agent-change-review.md) owns per-path
  attribution. This document does **not** read it — see "Why not change review".

## Context and Problem Statement

Two complaints about the same surface, both about the arrangement rather than
the contents.

**The centre says nothing.** Slot 0 belongs to the commons, and every other
district is seated by arrival order broken by facet similarity. So a district
that exists to coordinate others — an academic manager, an orchestrator — sits
wherever it happened to be created, usually in the outer ring because it was
created late. The lattice was built to make centrality expressible
(`ringLattice.ts`, "Why rings rather than a grid"), and then nothing was ever
given the job of expressing it.

**The map is emptier than its contents.** Ring radii are sized from unit
extents plus a 96-unit margin, while the ground is drawn at
`groundRadiusFor(extent, nearest)` — a *target* of 120 that gets clipped to
`nearest / 2 - 24` when the ring is tight. The two are not talking to each
other. A ring of one-agent districts has extents around 48, so its radial step
is `48 + 48 + 96 = 192` and every ground on it is squeezed to 72 — while a ring
sitting outside a large commons is pushed out so far that its grounds stay at
120 with several hundred units of grass between them. Same map, both failure
modes, opposite directions.

## Decision

### 1. Reach decides ring, within a district's own tier

Define a district's **reach** as the number of *other* districts whose workspace
roots its agents have written under. Not messages sent, not edges drawn — file
writes, because that is the one thing an agent does that survives it.

Reach is bucketed into tiers before it touches geometry, and the tiers are the
whole stability story:

| reach | tier | reading |
|---|---|---|
| 0 | 0 | works only in its own territory |
| 1–2 | 1 | reaches a neighbour |
| 3–5 | 2 | coordinates a group |
| 6+ | 3 | coordinates the map |

Slots are then sorted so that no district sits outside a district of strictly
lower tier. The sort is done by **swaps**, not by shifting: promoting one
district displaces exactly one other, rather than sliding everything between
them outward by a slot. `CENTER_SLOT` is excluded — the commons keeps the
middle, for the reason `placeDistricts` already gives.

Districts holding a cell but not currently active (tombstoned) are excluded from
the swap set. Their cell is reserved precisely so they land back where they were,
and swapping it away would defeat the reservation.

#### Why this does not break the stickiness invariant

`districts.ts` says the interior never moves, and the reason is that *insertion*
must be additive: adding an agent cannot be allowed to reshuffle a map the user
has learned. Reach is not insertion. A district crossing a tier boundary is a
change in what that district **is**, and the map moving to say so is the lattice
doing its job. The guards that keep it from becoming churn:

- **Tiers, not counts.** Reach is already an integer, and bucketing it means a
  district must gain a whole class of collaborators to move at all.
- **Fetched once per root set.** Reach is history. The hook does not subscribe
  to file events, so the map cannot rearrange itself while you are looking at
  it. The next launch adopts the new arrangement.

  Answered root sets are remembered, and that is load-bearing rather than an
  optimization: the fetch effect depends on whether the surface is enabled, so
  without it, hiding and re-showing the Garden re-read the archive and a
  cross-boundary write made in between could re-seat districts on return. A new
  look at the map is not a reason to adopt more history. A *failed* read
  releases its root set, so one transient error does not disable reach for the
  session.
- **Idempotent.** Cells are persisted. A sorted lattice re-sorts to itself, so
  the swap pass is a no-op on every session after the one that applied it.
- **Bounded.** At most one swap per misplaced district; the pass is a selection
  sort over occupied slots and terminates in `O(n²)` comparisons on `n` ≤ a few
  dozen districts.

#### Why not change review

`load_change_review` attributes a file to an agent by matching the *conversation's*
workspace against the repository — `read_turns_for_workspace` filters on
`same_workspace(cwd, entry.workspace)`. An agent that writes across a boundary
does so from a conversation rooted on its own side, so its writes never appear in
the other repository's attribution. Cross-district reach is structurally
invisible there.

It is also fetched per visible root. Feeding a viewport-scoped, live-updating
quantity into geometry would mean the map rearranges when you pan, which is the
exact failure `LayoutInput` exists to make untypeable.

So reach is read from the turn records directly, through a new command, over
every agent at once.

#### Why written paths rather than conversation workspaces

The conversation index carries a `workspace` per conversation and is far cheaper
to read. It was measured and rejected: on a 140-agent roster only seven agents
have conversations in more than one workspace, and six of those are the same
project's worktrees or a folder that was renamed. It reports approximately
nothing.

Written paths report the real thing. On the same roster, `Academic-Manager`
(workspace `.../academic`) writes into `.../hivemind` and
`.../onedrive/researchprojects/...`, both of which are other agents' roots.

Absolute paths that fall outside every known root — `.claude/`, `AppData/`,
provider scratch directories — are dropped by the root match itself, so no
exclusion list is needed and none is maintained.

### 2. Rings reserve what the ground asks for

Ring radii are now sized against each district's **drawn footprint**,
`max(extent, MIN_GROUND_RADIUS)`, with the gap that `groundRadiusFor` already
promises — `2 * GROUND_GAP` — in place of the old 96-unit margin.

This is one change with two effects, and they point opposite ways on purpose:

- Districts whose units spread wider than a ground move **closer**, because the
  gap between their footprints drops from 96 to `2 * GROUND_GAP`.
- Districts smaller than a ground move **apart**, from a 192-unit step to
  `2 * MIN_GROUND_RADIUS + 2 * GROUND_GAP` — and their ground grows from 72 to
  the full 120 to fill it.

Both land on the same invariant: **the clear space between two grounds is
`2 * GROUND_GAP`, everywhere.** The lattice reserves what the ground will draw,
so `groundRadiusFor`'s clip stops being the thing that decides ground size on a
crowded ring, and stops leaving hundreds of units of grass on a sparse one.

`GROUND_GAP` is therefore the one knob for how much grass the map shows, set at
16 for a 32-unit clearance. It buys less than it looks like where the floor
dominates: a ring of one-agent districts steps by `240 + 2 * GROUND_GAP`, so the
gap moves the pitch by a few percent and the floor decides the rest.

The remaining slack is **angular, not radial**. Ring `r` holds `6r` slots at a
radius set by clearing the ring inside it, and on a map several rings deep that
radius is wider than the ring's own contents need — neighbours within a ring end
up further apart than the margin asks for. Closing that means deriving slot count
from radius, which makes a slot index mean something roster-dependent: a
`RING_ARRANGEMENT` break that re-places every district, not a tuning change.
Deferred deliberately.

The floor applies to districts that draw no ground too — in practice only the
commons, whose extent exceeds it anyway. Threading "does this district have a
workspace" down into the lattice would buy nothing and would put a terrain
concept inside the geometry module.

## Interface

```rust
#[tauri::command]
pub async fn load_agent_reach(
    roots: Vec<String>,
    state: State<'_, AppState>,
) -> Result<AgentReachResponse, String>;
```

```rust
pub struct AgentReachEntry {
    pub agent_id: String,
    /// Roots this agent has written under, as spelled in the request.
    pub roots: Vec<String>,
}

pub struct AgentReachResponse {
    pub schema: u8,
    pub agents: Vec<AgentReachEntry>,
    /// Turn records that could not be parsed, so "no reach" is distinguishable
    /// from "could not read".
    pub skipped_turn_records: u64,
}
```

The caller supplies the roots. The backend owns path resolution and the frontend
owns the district mapping, because it is the only side that knows
`rootsByDistrict`. Self-reach is reported rather than filtered: which roots share
a district is not a question the backend can answer.

Path resolution is **lexical, and containment is segment-aware**. A relative
write is joined onto the conversation's workspace and *then* normalized, so `..`
resolves against the workspace it escapes from. Skipping that step made
containment a plain string prefix test, and `D:/dev/app/../other/x.ts` passed it
— a write into a sibling repository attributed to the workspace it left. Reach
seats districts, so that is not a cosmetic mismatch: it moves the map on evidence
that does not exist.

Three shapes have to be told apart for this to hold, and each was wrong before:
a UNC root is two segments; `D:x` is drive-*relative* and must be joined, unlike
`D:/x`; and a POSIX `/` must survive normalization rather than trimming to the
empty string and being discarded as unusable. `..` clamps at the root, matching
what the filesystem would have done.

Resolution stays lexical rather than canonicalizing on disk. These paths come
from turn records that may name files since deleted or volumes no longer
mounted, and canonicalizing would turn a history read into a disk walk.

Cost is bounded by `AGENT_REACH_CONVERSATION_LIMIT` (64) most-recent
conversations per agent. Measured on a 140-agent archive: 302 conversations
carrying turns, 54.7 MB, 0.34 s in Python. It runs once per session.

## Consequences

- One district's promotion moves two districts. That is visible and intended;
  it is also rare, being gated on a tier boundary.
- Reach is a lower bound. An agent that edits another project through a shell
  command Wardian did not record as a written path does not count, and the map
  understates rather than invents.
- A roster of uniformly small districts gets a wider lattice than before. Its
  grounds get proportionally larger, so the drawn map is fuller, not sparser.
- `RING_ARRANGEMENT` does not change. Slot indices still mean what they meant;
  only which district holds which slot can change, and only by a swap.

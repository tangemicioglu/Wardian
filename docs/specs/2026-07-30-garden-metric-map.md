# Garden as a Metric Map

- **Status:** Proposed
- **Date:** 2026-07-30

## Sources

- [Malleable Garden Design Philosophy](./2026-06-02-malleable-garden.md) defines
  Garden-owned versus canonically-owned state, the `EntityRef` direction, and the
  typed relationship vocabulary this spec implements a first slice of.
- [Entity-Oriented Agent Semantics](./2026-07-14-entity-oriented-agent-semantics.md)
  establishes that an entity is a projection across existing records rather than a
  new store, and that Garden spatial proximity stays interpretive.
- [Communication Topology](./2026-07-02-communication-topology.md) owns the
  agent-to-agent edge model this consumes.

## Context and Problem Statement

The Garden view is a proof of concept. Its positions came from a phyllotaxis
spiral, seeded per entity kind, because the alternative was worse: the graph
view's spring-electric simulation in `features/graph/graphProjection.ts` piled
agents on top of each other, and `gardenProjection.ts` documented that it was
deliberately discarding those positions.

Both approaches share a root problem. A spring-electric layout minimizes an
energy with no relationship to any semantic quantity, so rendered pixel distance
means nothing in particular. A spiral is worse still: it means nothing at all,
only guaranteeing non-overlap. Neither can absorb the entity kinds the Garden is
supposed to hold — library assets, artifacts, folders — because adding node kinds
to a force simulation produces a denser hairball, not a richer map.

The missing piece is not more node types. It is a **coordinate system**.

Three things the Garden is expected to be have incompatible position semantics:

| Surface | Position is | Stability |
| --- | --- | --- |
| Canvas | authored | absolute |
| Desktop | authored, snapped into named regions | absolute |
| Map | derived from a metric, but canonical | same object, same place, always |

These cannot be blended by tuning a force simulation. They blend by defining
position as a layered algebra where each layer has different authority.

## Proposed Decision

Position is derived from an explainable distance metric over canonical Wardian
records, laid out by stress majorization with an incrementality penalty, and
overridden by user placement that feeds back into the metric rather than fighting
it.

### Position algebra

```
pos(e) = pin( district( embed(e) + local(e) ) )
```

- **L0 Embedding** — derived from the semantic metric. Global, coarse, slow.
- **L1 Local layout** — deterministic arrangement and overlap resolution inside
  a district. Derived, warm-started.
- **L2 Authored** — pins and placements. Hard constraints that also *deform* L0.

The invariant: L2 wins, L1 never reorders L0, and L0 changes only when canonical
records change — never because a lens toggled, a status ticked, or a window
resized.

### The distance metric

There is no natural distance between an agent and a folder, so distance is
computed in facet space over a shared affiliation vocabulary. Three components
compose, each renormalized over the terms that actually apply to the pair:

| Term | Source | Why this form |
| --- | --- | --- |
| `d_affil` | weighted cosine over facet vectors | Heterogeneous kinds become comparable through shared scopes. |
| `d_interact` | personalized PageRank over `PairActivity` | Shortest path collapses hub neighbourhoods; PPR discounts hub-mediated adjacency and rewards independent paths. |
| `d_use` | PMI over co-use within an interaction thread | Raw co-occurrence re-ranks by popularity, making the busiest agent close to everything. |

Facets are weighted by inverse document frequency, which gives path-depth
weighting for free: deeper directories have fewer members, so a `gamma^depth`
term is unnecessary and the weighting adapts as the workspace grows. `kappa`
priors encode how much sharing a *kind* of thing should mean, independent of
rarity — sharing a provider is both common and unimportant; sharing a path is
common but important.

Explicit user negatives repel: `ignored_pairs` and `suppressed_seed_pairs` push a
pair out of each other's neighbourhood. A map where deleting a link moves nothing
teaches people to stop correcting it.

### The corpus is the materialized entity set, not the disk

Wardian has no recursive filesystem enumeration — `get_directory_tree` reads one
directory per call and the explorer expands lazily. Document frequency therefore
cannot span the filesystem without a crawl, and does not need to: files join the
corpus only when a folder parcel is expanded. Expansion increments `df` along the
affected ancestor chain, and the layout's drift penalty absorbs the local
perturbation. **Level of detail and data ingestion are the same boundary.**

### Districts

Districts are partitioned by a canonical, human-nameable record — team, then
workspace-fallback group, then worktree, then workspace path — never by
clustering, which one new agent can re-partition. Cells are assigned along a
Hilbert curve because it preserves locality in both directions, and cells are
**sticky**: once a district owns one it keeps it, with a TTL tombstone so an
emptied district that returns lands where it was.

Districts are also a computational firewall. They cap `n` in every superlinear
stage, which is what confines insertion cost to one district instead of the map.

### Layout

Stress majorization (SMACOF) replaces the spring simulation, minimizing

```
sigma(X) = sum w_ij (||x_i - x_j|| - d_ij)^2  +  sum rho_i ||x_i - p_i||^2
```

with `w_ij = d_ij^-2`. The first term makes rendered distance converge to
semantic distance — the difference between a graph drawing and a map with a scale
bar. The second is the incrementality mechanism: warm-started from stored
positions, insertion perturbs a neighbourhood and leaves the rest in place.

`rho` is expressed *relative* to each node's total stress weight. Absolute
stiffness is dimensionally wrong: stress weights scale with world units, so a
`rho` of 1.0 would outweigh every distance target by three orders of magnitude
and freeze the layout at its seed.

Overlap removal uses separation constraints (VPSC) rather than push-apart,
because push-apart reorders neighbours and the map visibly changes shape when a
label grows. Constraint *direction* comes from a total order fixed once from the
incoming layout; orienting by moving positions lets later constraints contradict
earlier ones, and the resulting cycles are unsatisfiable.

### Malleability

Three tiers of user authority:

1. **Nudge** — a fixed point in the solver.
2. **Pin** — durable, and stored **district-relative** as `(district_id, dx, dy)`.
   Absolute pins rot: if a district's cell shifts, the entity is stranded in the
   wrong neighbourhood and the map starts lying about affiliation. A pin whose
   entity changes district is explicitly invalidated and surfaced, never silently
   honoured or silently dropped.
3. **Anchor** — placement writes a `scene_anchor:<district>` facet that
   participates in the metric like any other. Placing A in P genuinely makes A
   closer to P's members, so its neighbours, ranking, and future placement follow
   without a pin. Exclusion is symmetric via repulsion.

Anchor weight is capped and time-decayed so placement never compounds with
repetition and fades if the scene is abandoned — otherwise the map ossifies
around one afternoon's arrangement.

Per the entity-semantics spec, `scene:*` facets change geometry and ranking only.
They create no team membership, deployment, or binding. The namespace split makes
that checkable rather than a convention people drift from.

### Explainability is a constraint on the metric

Cosine decomposes linearly over shared facets, so per-facet attribution costs one
extra pass. This is a requirement, not a nicety: a map whose distances cannot be
interrogated is a lava lamp. It is also the reason to reject UMAP/t-SNE-style
embeddings, on top of their nondeterminism and instability under insertion.

## Districts are sized, not assumed

The grid pitch was a constant while overlap removal ran per parcel with no
notion of a cell boundary, so a populous district simply grew past its cell.
Measured on synthetic rosters, a 24-member district spans ~1250 world units
against a 720 pitch: neighbours overlapped by ~500, and the map showed one
crowd where the data had two. Bleed began around a dozen members per district,
well inside normal use.

Each district is therefore solved in its **own frame**, centred on the origin,
measured, and only then translated onto the grid — the pitch is derived from the
widest district rather than hoped for. Three consequences worth stating:

- The pitch is **persisted in the scene**. A stored position is absolute, so
  without the pitch that produced it a later pass cannot recover which
  district-relative point it represents, and every warm start would be silently
  offset.
- It is **quantized with hysteresis**: growth is immediate, shrinking waits for
  a whole step. The pitch feeds back into the next pass through warm starts, so
  an ungated rule would creep outward on every relayout.
- **Pins do not count toward the measurement.** A pin is authored placement,
  which outranks the metric by design; dragging one unit toward the edge is not
  a request to move every district apart. It would also ratchet — a wider pitch
  moves the district origin, which moves the pin, which widens the pitch.

## What places a workflow

A blueprint binds no agent until a run assigns roles, so workflows were parked
in the commons — where, carrying one facet each, they were mutually
indistinguishable and piled up. The premise was wrong: a blueprint is not short
of evidence, nobody had read it.

Three signals, strongest first, all from the blueprint's own content:

| Signal | Source | Strength |
| --- | --- | --- |
| Agent binding | `agent_ref` field on a `task`/`decision` node | canonical link |
| Workspace path | `path` field — a `shell` node's `cwd`, a `script` node's `path` | shared with agents that reach it |
| Library folder | `workflows/<folder>/` | groups a family |

Which fields count is read from the **node registry**, which declares each
field's `kind`. That matters: a `shell` node's `command` often contains
something path-shaped, and a name-based allowlist would read it as a directory.

Placement by path is an IDF-weighted vote over the facets a workflow shares with
agents, using the same smoothed statistic as the metric. A workflow whose shell
node runs in `D:/Trading/trident` shares that facet with the two agents living
there — `ln(54/3) ≈ 2.9`, decisive — while `path:d:/` is on every agent, so
`df == N` makes it worth exactly 0. No rule anywhere has to know that a drive
root is uninteresting and a project directory is not. Below a floor score the
workflow stays in the commons, because a placement on thin evidence is a guess
dressed up as a derivation.

## Stability Contract

> Adding or removing an entity moves no other entity more than delta, unless the
> change alters a district assignment.

Three things may legitimately break it, and each must be visible:

1. District reassignment — animate the transit.
2. A rare facet becoming common — meaningful; a skill went from bespoke to standard.
3. A metric version bump — the scene records `metric_version`; a mismatch offers
   re-derivation with a preview rather than reflowing silently.

Everything else — telemetry, status, messages, lens toggles, resizes — changes
colour, tint, and emphasis only. This is enforced by a type boundary:
`LayoutInput` accepts no telemetry, so geometry cannot depend on it.

Keeping geometry cheap is not the same as keeping *rendering* cheap, and the two
were confused. Telemetry ticks rebuild every unit so status stays live, and each
rebuild used to re-render every unit and re-resolve every colour through
`getComputedStyle` — dozens of forced style recalculations per tick. Worse, the
status pulse ran a `requestAnimationFrame` loop **per active unit**, each driving
a React state update, so a busy agent reconciled its entire skill crown once per
frame to move one circle by a pixel. Three rules now hold the line:

- **Canvas animation belongs on the canvas.** One `Konva.Animation` scales every
  tagged halo and the layer redraws once. The pulse costs nothing in React and
  does not scale with what else a unit draws.
- **Resolved colours are cached per theme.** The theme name is part of the key,
  so a swap self-invalidates without coordination — reading an attribute is free,
  reading a computed style is not.
- **Units compare their props field by field.** Prop identity always changes, so
  the default shallow comparison would never skip anything; `position` and
  `crown` come straight from the layout result and are compared by reference.

### Districts are arranged around a centre, not enumerated onto a grid

Districts sat on a Hilbert-curve grid, chosen because the curve preserves
locality: grid neighbours are curve neighbours, so placing a new district near a
similar one was a search along an index. That property was worth having and is
kept. What the grid could not express is *centrality*. Every cell is equivalent,
so the commons — the shared pool that unaffiliated entities and workflows fall
back to — was wherever the curve happened to put it, which is a corner. A map
whose shared centre reads as peripheral is asserting something false.

Districts now occupy slots on a concentric ring lattice. Slot 0 is the origin and
belongs to the commons unconditionally, reserved even when the commons is
briefly unpopulated, so arrival order cannot win the middle and then keep it
forever. Ring `r` sits at radius `r · spacing` and holds `6r` slots, which makes
the arc between neighbours

    2π(r · spacing) / 6r  =  π · spacing / 3  ≈  1.047 · spacing

independent of `r` and equal to the radial gap between rings — a hexagonal
packing in polar clothing. One pitch still governs the whole map, so `spacingFor`
and the district-sizing rule above carry over untouched. Odd rings are staggered
by half a step so districts do not line up into visible spokes.

Placement is otherwise unchanged: a new district takes the free slot minimizing
its similarity-weighted distance to those already placed, so semantically close
districts end up adjacent. Two properties come free. Rings are unbounded, so the
grid-exhaustion case — which parked a district on top of the commons — no longer
exists. And ties break on the lower slot index, which now means *closer to the
centre*, so the map fills outward instead of trailing into a ring of its own.

Slot indices are persisted, so their meaning is part of the stored format.
`DistrictLayout.arrangement` records which mapping assigned them; a mismatch
re-places from scratch rather than reinterpreting, because reading a Hilbert
index as a ring slot would not relocate districts so much as scatter them to
positions that never meant anything. Pins survive regardless — they are
district-relative offsets, which is precisely why they are stored that way.

Measured on the 53-agent fixture: 37 districts, which is rings 0–3 filled
exactly (1 + 6 + 12 + 18), a 5403 × 7085 map at a pitch of 960, fitting at 0.144.
The grid produced 5229 × 6125 at 0.17 — a circular envelope costs a little
bounding box, and buys a centre that means something.

### What binds a workflow to an agent

A blueprint says what a workflow *needs*, not what it got. `role:evolver` names a
role; which agent fills it is decided when the workflow is deployed, and that
decision lives in the schedule record rather than in the document. Reading only
the blueprint, the Evolver's three workflows shared nothing with the Evolver but
a word, and sat in the commons while the agent they run on sat elsewhere.

So an `agent_ref` is read as three different things:

- A bare value is an **agent id** and binds the workflow to that agent.
- `role:name` and `class:name` are **unfilled requirements**. They are recorded
  as facets, because workflows wanting the same role are genuinely alike and the
  affinity fallback can use that, but they cannot place a workflow: they name a
  kind of agent, not one.
- `ephemeral` names a throwaway and ties the workflow to nobody.

The binding itself comes from the schedules that deploy the blueprint, pooled
with any ids the blueprint names outright. This is the same standing as a skill's
deployment or an artifact's origin — a canonical record of where something
actually runs — and it is what `deployed:agent:` facets have always meant. Only
`target_type: "agent"` counts; a `temporary_provider` exists for the length of one
run and belongs nowhere. On the reference install this gives six blueprints a
real district, including all three Evolver workflows.

### A stored position is only meaningful in the frame it was written in

Warm starts exist so the drift penalty has an anchor, which is what makes
inserting one agent an incremental change rather than a full reflow. They are
derived state, and their authority stops there — a stale one is worse than none.

Positions are stored absolutely, so they mean nothing without the origin they
were measured from, and that origin is a district's. When districting changes,
yesterday's coordinate re-based on today's origin is off by roughly the distance
between two cells. Nothing about that difference is a memory of anywhere. It is
an artefact, and under the drift penalty the unit holds it — so the district's
measured extent grows by a whole grid pitch, and the pitch is *derived from that
extent*. The next pass then reads every stored position in a wider frame, and the
error compounds across sessions because the pitch is persisted. Measured: one
re-districting took the pitch from 720 to 7440; a few in succession put a real
53-agent map at a pitch of 9,402,240 and a span of 28.5M × 51.7M world units,
which no viewport can show. The map was not empty. It was unreadably large.

Three rules contain it:

- **The scene records which district each position was measured in**
  (`position_districts`). A warm start whose district has changed is discarded
  rather than reinterpreted. This is exact, and it prevents the first inflation.
- **A warm start beyond `MAX_DISTRICT_RADIUS` of its origin is discarded**
  regardless. The district check cannot recognise a scene that is already
  inflated, because such a scene is internally consistent — the position really
  was written under this district, in a frame that was already wrong. An absolute
  bound is the only thing that can, and it lets an affected scene heal on load
  instead of asking the user to discard their arrangement.
- **The pitch is capped at `MAX_DISTRICT_SPACING`**, and a persisted pitch above
  the cap is never a reason to stay there. Past the cap districts may touch,
  which is a far smaller failure than a map too large to draw.

The general rule: geometry derived from persisted geometry needs a bound that
does not itself come from the persisted value, or a single bad measurement
becomes permanent.

## Identity

The Garden must treat five live key schemes as one keyspace: `unitKey`,
`entry_ref`, `folderKey`, `fileResourceKey`, and `Blueprint.id`. A list view
tolerates that fragmentation; a map does not, because the same object arriving
under two keys renders as two units at two positions with two facet vectors.

The concrete collision is workflows, which carry `Blueprint.id` *and* library
`entry_ref = workflows/<file>.md`, reconciled today by ad-hoc path matching in
`detail/WorkflowDetail.tsx`. `EntityRef` canonicalizes on `Blueprint.id`.

`memory` is deliberately absent from the entity kinds: no memory feature,
command, or DTO exists, and every reference to it is aspirational spec language.
`artifact` is the implemented analogue and carries provenance to its producing
agent.

## What earns a unit

Having a canonical identity is not sufficient to have a position.

> An entity that is an **attribute** of another entity renders *on* it. An entity
> with **independent existence and its own lifecycle** gets a unit.

| Entity | Independent lifecycle | Encoding |
| --- | --- | --- |
| Agent | yes | unit |
| Workflow blueprint | yes — editable, has runs | unit |
| Skill *deployment* | no — a fact about an agent | glyph on the agent |
| Skill *itself* | yes, but lives in the Library | reverse highlight, never placed |
| Class | no — an agent has exactly one | district fallback tier |
| Prompt | attaches to nothing | absent |

The failure this rules out is concrete. A skill deployed to six agents is one
object that must sit in one place, so the metric puts it at the centroid of its
targets — a location where it is relevant to nobody — and "pick the
most-referenced district" is a tie-break for an unanswerable question. The error
is upstream of the layout: `deployed:agent:a1` is a fact about a1, not about a
location. This is the same reasoning that already turns containment and team
membership into geometry rather than nodes; deployments were the case that had
been missed.

Instancing a skill across its carriers gains one thing the unit model could not
express at all: a skill deployed to a *class* has no single agent to sit beside,
so class-inherited capability was previously unrepresentable. It costs one
thing, and that cost must be paid back — a skill no longer has a place to
navigate to, so "where is this used?" is answered by highlighting the carrying
set. A set is the honest answer to that question; a point never was.

Three constraints follow:

- **Glyphs never enter the layout.** They are decoration attached to a position,
  which is what lets detail expand and contract with zoom. An agent's overlap
  footprint grows with the number of skills it carries, measured at full detail
  rather than at the current zoom — a footprint that shrank when you zoomed out
  would make geometry a function of the viewport. An agent with no skills
  reserves nothing.
- **Skills stay in the metric.** `skill:<entry_ref>` on the *agent* is rare and
  therefore high-IDF, so two agents carrying the same unusual skill are pulled
  together. Leaving the unit set strengthens this: there is no longer a
  cross-kind offset working against the district assignment.
- **The crown is ordered by IDF descending.** A skill deployed to every agent
  renders on every agent; at `df == N` its IDF is exactly 0, so it sinks into the
  truncated tail without a special rule. The crown shows what is distinctive
  about an agent rather than what is ubiquitous.

Glyph identity is the weak point. Dwarf Fortress symbols work because the set is
fixed and learned; Wardian skills are user-named, so no generated glyph set can
be distinguishable in general. The fallback is a monogram plus a hash-derived
hue, assigned globally so one skill means one glyph everywhere, with collisions
resolved against the last word — which is where near-duplicate names actually
differ. A user-assigned icon in `LibraryItemMetadata` is the intended escape
hatch and is not yet built.

## Consequences

- **Positive:** Distance means something and is explainable per facet.
- **Positive:** Insertion touches one district; nothing global recomputes.
- **Positive:** User placement is first-class and informs the metric instead of
  fighting it.
- **Positive:** Heterogeneous kinds can enter without producing a hairball,
  because containment and affiliation are geometry rather than drawn edges.
- **Negative:** More moving parts than a force simulation, and three of them
  (SMACOF, VPSC, Hilbert placement) are hand-rolled because the project has no
  d3/elk/dagre dependency.
- **Negative:** VPSC omits block splitting, so displacement is occasionally
  conservative rather than optimal. Feasibility is preserved by a fixed-point
  loop plus a longest-path fallback.
- **Negative:** Scenes still persist through browser storage rather than as
  inspectable files under the Wardian home.
- **Negative:** Monogram legibility at 8px is unproven against real skill names,
  and near-duplicates are common in practice. The user-assigned icon that would
  settle it does not exist yet.

## Deferred

- Scene files under the active Wardian home. The scene is I/O-free, versioned,
  and has a tolerant reviver specifically so this is a storage swap.
- CLI parity: `garden near`, `garden explain`, `garden district`. All three fall
  out of the metric with no extra machinery and are testable without a renderer.
- The aggregation tree and level-of-detail budget, which is what admits folders
  and the filesystem at scale.
- Power-diagram parcels with real borders and drop targets; parcels are currently
  positional only.
- Edge rendering policy: only flow relations should draw lines, bundled along the
  aggregation tree. Structural and affiliation relations are already expressed as
  geometry.
- Interruptible layout across animation frames. `smacofStep` supports batching;
  the view currently runs to convergence synchronously.
- A user-assigned skill icon in `LibraryItemMetadata`, overriding the generated
  monogram. The metadata store is already keyed by `entry_ref`, so this is a
  field plus an editor.
- An expanded selection panel listing an agent's skills by name and grouped by
  provenance. The crown answers "what kind of agent is this"; naming every skill
  is a closer-range question the summary bar currently only counts.

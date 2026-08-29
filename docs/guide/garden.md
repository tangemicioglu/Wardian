# Garden

The Garden is a map of everything Wardian knows about, laid out so that
position means something. Agents, automations, skills, and library assets all
appear as units, and two units sit near each other because they share facts —
the same workspace, the same team, the same worktree, the same deployment.

Use the Garden when you want to see the shape of your setup: which agents
cluster around a repository, which automations belong to which agent, and what is
sitting on its own with nothing tying it to the rest.

## How Position Is Decided

Each unit is described by **facets** taken from records Wardian already keeps —
a workspace path and every directory above it, team membership, agent class,
provider, worktree, the agents a skill is deployed to, the agents an automation is
bound to. Two units are close when they share facets, and a shared facet counts
in proportion to how rare it is. Sharing a provider with twenty other agents
says almost nothing about you; sharing a worktree with one other agent says a
great deal.

Nothing about position is arbitrary or remembered from a previous session's
accident. Select a unit to see the status summary for it; the layout is derived
fresh from the current roster, library, and schedules every time those change.

## Districts

Every unit belongs to a **district**, chosen by the strongest tie it has, in
this order:

1. **Team** — the agent is in a team.
2. **Fallback** — an explicit override recorded for the agent.
3. **Worktree** — the agent runs in a git worktree of a repository.
4. **Workspace** — the agent runs in a folder no other tie describes.
5. **Commons** — nothing ties this unit anywhere in particular.

The commons sits at the centre of the map, and the other districts are arranged
in rings around it, sized to hold what is actually in them. Districts that are
semantically close end up as neighbours rather than being spread out by
alphabetical or creation order.

An automation lands in the district of the agent it runs on. That agent can come
from the blueprint itself, when a node names a specific agent, or from the
schedule that deploys the automation. A blueprint that only says `role:evolver` or
`class:Coder` has stated a *requirement* rather than a binding — it says what
kind of agent it needs, not which one — so an automation like that stays in the
commons until a schedule deploys it somewhere.

## Navigating the Map

- **Scroll** to zoom. Zoom uses the wheel delta, so high-resolution trackpads
  move continuously while a normal wheel notch remains a small step. It is
  anchored at the pointer, so the point under the cursor stays put.
- **Drag the background** to pan.
- **Arrow keys** to pan; hold Shift for larger steps.
- **`+` / `-`** to zoom in and out.
- **`0` or `f`** to fit the whole map into view.

The current zoom level is shown in the bottom-right controls, next to buttons
for zooming and for **Fit**. The zoom readout is worth checking first if the
canvas ever looks empty — it usually means the view is scrolled away from the
content rather than that there is nothing to show.

## Moving Units by Hand

Drag a unit to place it where you want inside its own district. The position is
saved and survives restarts and re-layouts.

A drag adjusts where a unit sits *within* its neighbourhood; it does not change
which neighbourhood the unit is in. District membership comes from the canonical
records above, so dropping a unit beyond its district's edge places it at the
edge rather than moving it into the district next door.

Use **Reset layout** to discard every hand placement and return to the derived
layout.

## Where the State Lives

The Garden's derived layout is not a source of truth for anything. Districts,
distances, and clustering are all computed from records that already exist:
the agent roster, `topology.json`, team membership, the automation library, and
schedule records. Change any of those and the map follows.

The only Garden-owned state is what you place by hand, along with the current
camera position, and it is stored per-district so that it stays meaningful when
districts are rearranged around it.

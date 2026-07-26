# Contextual Agent Surface Targeting

**Date:** 2026-07-24
**Status:** Implemented

## Decision

Agent-open actions from Graph, Garden, and Inbox first reuse the existing
singleton **Agents** surface wherever it lives in the Workbench. Wardian
focuses that pane, activates its tab when needed, selects the requested agent,
persists it as the surface's focused agent, and scrolls the card into view
without adding an Agent Session tab. If no Agents surface is available, a
directly adjoining visible Agent Session remains the fallback target and is
rebound to the selected agent.

This makes a two-pane investigation layout behave as one working context:
choose an agent in a discovery or notification surface, then inspect that
agent in the Agents or session presentation already placed beside it.

## Eligibility

The existing Agents surface can be in the invoking pane or in any other pane,
and it can be an inactive tab. When the Workbench is zoomed, only an Agents
tab in the zoomed pane is eligible, so an open action does not unexpectedly
switch to a hidden pane.

The Agent Session fallback derives normalized pane bounds from the persisted
split tree. Its target must meet every condition below:

1. It is the active tab of a different pane, so it is visible.
2. It is an `agent-session` surface.
3. Its pane shares a non-zero edge with the invoking pane.
4. The Workbench is not zoomed to one pane.

If more than one eligible pane shares an edge, Wardian selects the one with
the longest shared boundary. A tie remains deterministic in split-tree order.

## Fallback and Safety

If there is no eligible target, Wardian uses the existing resource-aware
focus-or-open behavior. A contextual open does not alter the pane tree, create
a duplicate presentation, or issue any agent lifecycle command.

Agent Session rebinding uses the same guarded close transaction as an explicit
rebind. If that transaction is cancelled or becomes stale, the existing
presentation remains unchanged and Wardian does not create a replacement tab.

## Scope Boundaries

- The right roster keeps its explicit **Open** and **Open to Side** semantics.
- Files and artifacts are excluded because replacing a visible editor could
  conflict with dirty-buffer intent.
- Workflow-to-agent targeting is deferred until workflows expose a stable,
  unambiguous execution inspection target.

## Verification

- Unit tests cover adjacent-pane detection, inactive-tab exclusion for the
  Agent Session fallback, existing Agents lookup, and deterministic selection
  among multiple neighbors.
- Navigation tests cover rebind-without-new-tab, ordinary fallback, and the
  zoomed-pane fallback.
- App and browser integration verify that a Graph action activates an existing
  Agents tab in both the invoking and another pane before falling back to an
  adjoining Agent Session, without triggering an agent lifecycle operation.

# Global Agents View Routing

**Date:** 2026-07-25
**Status:** Implemented

## Decision

An **Open Agent** action from Graph, Garden, or Inbox treats an existing
**Agents** surface as the primary destination throughout the Workbench. The
surface can be active or inactive, in the invoking pane or another pane, and
is selected using the Workbench's most-recently-used surface order.

If the target is outside a currently zoomed pane, Wardian restores the normal
split layout before activating the target tab. This makes the navigation result
visible and preserves the investigation context instead of opening a duplicate
Agent Session tab.

## Resolution Order

1. Find an existing Agents surface anywhere in the Workbench.
2. Restore pane zoom only when it hides that surface.
3. Focus its pane and tab, select the requested agent, persist the focused
   agent ID, and scroll its card into view.
4. If no Agents surface exists, retain contextual Agent Session rebinding for
   a visible adjoining session; otherwise use the ordinary Agent Session open
   policy.

## Boundaries

- The roster's explicit **Open** and **Open to Side** commands continue to
  create or focus Agent Session presentations. They are deliberate terminal
  navigation, not a contextual reveal.
- The rule applies to agent-open actions only. It does not replace dirty file
  editors or alter an agent's lifecycle.
- Restoring zoom is runtime-only presentation state; it does not rewrite the
  persisted Workbench layout tree.

## Verification

- App integration covers inactive Agents tabs in both the invoking pane and a
  different pane, plus an Agents target hidden by source-pane zoom.
- Browser E2E clicks Graph's real **Open Agent** control after zooming its
  pane, then verifies the layout is restored, the existing Agents tab is
  active, the requested card is focused, and no Agent Session tab was added.

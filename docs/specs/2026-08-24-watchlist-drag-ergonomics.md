# Watchlist Drag Ergonomics

- **Status:** Implemented
- **Date:** 2026-08-24

## Context and Problem Statement

Reordering agents in the roster uses a mouse-event drag implementation rather
than HTML5 drag-and-drop, because WebView2 does not deliver a usable native drag
lifecycle. That implementation worked but felt coarse in three specific ways:

1. **Every press looked like a drag.** `onMouseDown` immediately set the drag
   source and applied `opacity-50` to the pressed row, so an ordinary click to
   select or reveal an agent flashed the dragging treatment. The same held for
   watchlist tab pills.
2. **The drop indicator lagged the cursor.** Each row's `onMouseMove` wrote the
   drop target to React state on every single event, even when the target had
   not changed, re-rendering the whole roster many times per second. The
   indicator itself swapped `border-top`/`border-bottom` from 1px to 2px, which
   shifted every row below it by a pixel, and `.watchlist-row` animated
   `transition: all 0.15s ease`, so the indicator faded in a frame behind the
   pointer while the rows jittered.
3. **A drag could not leave the viewport.** With more agents than fit the panel,
   moving a row to a position that was scrolled out of view was impossible in a
   single gesture: the roster never scrolled while the pointer was held down.

## Proposed Decision

### Press versus drag

A press *arms* a drag; it does not start one. `beginDrag` records the pointer
origin and the drag source, and the drag becomes live only when one of two
things happens:

- the pointer travels `DRAG_ACTIVATION_DISTANCE` (4px) from the origin, or
- the pointer reaches a drop target other than the row it started on (another
  row, a team block, or a team edge zone).

Only a live drag applies the dragging treatment (`opacity-50` on the source row,
`watchlist-dragging` on the roster scroller, the grabbing cursor). The second
activation rule keeps the existing drop semantics intact for a gesture that
crosses rows without a measurable intra-row movement, which is also how the
component test suite drives drags. Watchlist tab pills follow the same rule via
`isTabDragging`, set when a tab drag first targets a different tab.

### Fewer renders, no reflow

`setDropTarget` and `setTabDropTarget` compare against the current ref
(`isSameDropTarget` / `isSameTabDropTarget`) and return early when nothing
changed, so mouse movement inside one half of one row costs zero renders. The
indicator moved from a border-width swap to `box-shadow: inset`, which paints in
the same geometry and therefore never reflows the rows underneath, and the row
transition list is now explicit (`background-color`, `border-color`, `opacity`)
instead of `all`.

### Edge auto-scroll

`useDragAutoScroll(containerRef, active)` (in
`src/layout/watchlist/dragAutoScroll.ts`) drives the roster while a drag is
live. It tracks the pointer with a **capture-phase** window `mousemove`
listener, because watchlist rows call `stopPropagation()` in their own move
handlers, which halts the native event at React's root container before a
bubbling window listener would see it.

A `requestAnimationFrame` loop mutates `scrollTop` directly, so scrolling costs
no React renders. Velocity comes from the pure `computeAutoScrollSpeed`:

| Pointer position | Result |
| --- | --- |
| Middle band of the container | `0` |
| Inside the 56px edge zone | Quadratic ramp toward `maxSpeed` |
| At or past the container edge | `±900 px/s` |

The quadratic ramp means dipping into the hot zone creeps and burying the cursor
past the edge runs at full speed, which keeps a long roster controllable. The
edge zone is clamped to half the container height so short panels keep a neutral
band, and per-frame deltas are clamped to 50ms so a backgrounded tab cannot
resume with one large jump.

Chromium recomputes hover state after a programmatic scroll, so rows sliding
under a parked cursor fire `mouseover`/`mouseenter` and the drop indicator keeps
tracking without any extra hit-testing. This is asserted in the browser E2E
rather than assumed.

## Consequences

- **Positive**: Clicking a row no longer flashes the dragging treatment, and the
  drop indicator tracks the cursor without lag or row jitter.
- **Positive**: A drag can reach any position in the roster in one gesture,
  including positions that are scrolled out of view.
- **Positive**: The scroll loop and the drop-target dedupe both remove per-event
  React renders from the drag hot path.
- **Negative**: A press that moves more than 4px before release now begins a
  drag, so a very shaky click can produce a drop instead of a selection. 4px is
  the conventional threshold and was chosen over a time-based hold because a
  hold delay would make deliberate reordering feel slower.
- **Negative**: Auto-scroll depends on Chromium re-firing hover events after a
  programmatic scroll for the indicator to track. A browser E2E covers this;
  jsdom cannot, because it has no layout.

## Verification

- `src/layout/watchlist/dragAutoScroll.test.ts` — velocity ramp, edge clamping,
  degenerate containers.
- `src/layout/watchlist/AgentWatchlist.test.tsx` — activation threshold, jitter
  rejection, cross-row activation, teardown, tab dimming.
- `e2e/tests/watchlist-drag.spec.ts` — real layout and animation frames: a press
  without movement never enters the dragging state; a drag parked at the bottom
  edge scrolls the roster and keeps the drop indicator current; reversing to the
  top edge scrolls back.

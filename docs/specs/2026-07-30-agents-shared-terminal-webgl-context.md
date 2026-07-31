# Agents shared terminal WebGL context

## Status

Implemented. Browser and production context-cardinality acceptance is complete;
native acceptance remains required before merge.

## Problem

Agents Overview currently creates one xterm `WebglAddon` and therefore one
browser WebGL context for each GPU-backed terminal card. Wardian limits that
pool, demotes cards to xterm's DOM renderer, captures still-image overlays,
explicitly loses retired contexts, and later promotes cards again. Those
controls bound the number of live contexts, but they do not remove context
churn. Scrolling, renderer residency changes, maximize/restore, surface
visibility, and context loss can still switch a presentation between renderer
instances or backends.

The repeated lifecycle work is coupled to several rounds of scrollback and
snapshot recovery code. Those fixes established authoritative PTY history and
presentation recovery, but they cannot make independent canvas-bound WebGL
contexts stable. The Agents grid needs one GPU context whose lifetime belongs
to the surface rather than to any terminal card.

Standalone Agent Session terminals do not exhibit this failure mode. Replacing
their renderer would increase risk without addressing the reported problem.

## Decision

Each mounted Agents Overview surface owns one persistent canvas and one real
WebGL2 context. Every resident terminal card keeps an xterm instance, but the
Agents path does not load xterm's canvas-bound `WebglAddon`. A surface
compositor reads each xterm's public buffer API, rasterizes each visible
terminal body into a CPU-side tile, and composites those tiles through one GPU
draw pipeline.

The compositor owns pixels only. xterm remains responsible for:

- VT parsing and buffer state;
- terminal cell attributes and public buffer inspection;
- selection state, link hit-testing, keyboard, mouse, and accessibility
  behavior;
- terminal resize and render invalidation.

Wardian maps xterm buffers to Agents card rectangles, resolves terminal cell
attributes against Wardian's xterm theme, rasterizes standard and custom
glyphs into card-local Canvas2D tiles, clips each terminal to its card, and
schedules whole-surface recomposition after terminal or layout changes.
Card-local xterm DOM renderers remain mounted behind the compositor as the
complete interaction, accessibility, and fallback layer.

Standalone Agent Session surfaces keep the existing dedicated xterm WebGL
path. Remote terminals, the user terminal, Graph, Garden, PTY transport, and
the terminal presentation broker are unchanged.

## Architecture

### Surface-owned context

`AgentsOverviewView` mounts a shared terminal canvas beside the scrolling grid.
The canvas covers only the visible Agents viewport, uses device-pixel backing
dimensions, and has `pointer-events: none`. Card-local xterm elements continue
to own focus, input, selection, links, wheel handling, and accessibility.

The context owner survives card residency changes, scrolling, maximize and
restore, and short workbench visibility changes. It is disposed only when its
Agents Overview surface is destroyed. A hidden surface stops drawing without
destroying and recreating the context.

The owner tracks a stable viewport registration for each presentation:

- presentation ID;
- xterm instance and public active buffer;
- terminal host element;
- CSS and device-pixel bounds relative to the surface viewport;
- visibility and clipping state;
- terminal theme, cursor, selection, and invalidation state.

Scroll, resize, density, grid, card-mode, maximize, and device-pixel-ratio
changes invalidate the viewport map. Recomposition clears the shared canvas,
updates every visible viewport, then draws each visible buffer in deterministic
card order.

### Shared compositor

The compositor uses one WebGL program set and one vertex layout. A
presentation contributes one CPU raster tile and one texture within the
surface context, never a context of its own. Per frame, the compositor:

1. Reads the presentation's active xterm buffer at its public `viewportY`.
2. Intersects the terminal body with the visible Agents viewport.
3. Rasterizes background, selection, decoration, cursor, and glyph pixels into
   the presentation tile.
4. Uploads the dirty tile, applies one scissor rectangle, and submits its quad.
5. Moves to the next visible presentation without changing contexts.

Cell colors come from xterm's public color modes and Wardian's existing
`WardianTerminalTheme`. The renderer must support default, 16-color, 256-color,
and RGB foreground/background colors; bold, dim, italic, inverse, invisible,
underline, overline, and strikethrough attributes; wide and combined cells;
selection colors; and the configured cursor style.

Glyph rasterization uses Canvas2D text plus explicit block-element fills in a
tile uploaded to a texture in the shared context. Box-drawing and powerline
symbols use the configured terminal font. If native provider evidence finds a
material mismatch, Wardian may adapt xterm's MIT-licensed custom-glyph drawing
routines into a small attributed module. Such code must remain isolated from
broker and terminal-session logic and carry the upstream license notice.

`Terminal.onRender`, `onScroll`, `onResize`, `onSelectionChange`, title/theme
changes, and surface layout invalidation mark either one presentation or the
whole surface dirty. Multiple invalidations coalesce into one animation frame.
Disposing a card removes only its registration and cached draw data; it does
not delete surface GPU resources or affect the real context.

### Layering and interaction

The shared canvas paints terminal backgrounds, selections, glyphs, and cursors
into terminal-body rectangles only. Card chrome remains ordinary React DOM.
The compositor canvas is `pointer-events: none`, so xterm's textarea, viewport,
helper elements, selection mechanics, and link hit testing remain interactive
beneath it. The visible terminal rectangle excludes xterm's scrollbar gutter.

The surface wrapper owns the canvas as a sibling overlay above the scrolling
grid. Its backing size equals the visible wrapper, never the grid's full scroll
extent. Card rectangles are measured relative to the wrapper after scroll and
layout. The canvas is transparent outside terminal bodies, so it does not
cover card headers, gaps, menus, resize handles, or non-terminal cards.

xterm's link provider remains the authority for link identity, hover
decoration, and activation. Wardian does not create a second link parser. While
the platform link modifier is held over a terminal, the shared canvas becomes
transparent and exposes xterm's exact DOM interaction layer; releasing the
modifier restores and recomposes the shared surface.

No terminal DOM node may become the scroll owner for Agents Overview. The
existing xterm viewport remains the scrollback model, while the Agents surface
remains the grid scroll owner.

### Context loss and fallback

One lost shared context affects all terminal cards in that Agents surface, so
the failure must be atomic and visible:

1. Stop shared draws and hide the compositor canvas.
2. Reveal xterm's complete DOM renderer for every resident card.
3. Allow the surface canvas's native restoration event to rebuild programs,
   buffers, and textures once.
4. After restoration, mark every visible presentation dirty and reveal the
   compositor only after one complete frame.

If WebGL2 is unavailable or restoration fails, the Agents surface remains on
the DOM renderer for its lifetime. It must not enter a create, lose, retry loop.

### Existing renderer budgets

The 24-xterm residency budget remains. It limits parser/DOM/GPU resources for
large rosters and keeps the broker's mounted/suspended contract unchanged.

Agents terminals no longer participate in the process-wide 12-context LRU.
Their surface context counts once regardless of resident card count. Dedicated
standalone terminals continue using that LRU until a separate measured need
justifies changing them.

The Agents path removes per-card WebGL promotion, demotion, grace timers, and
snapshot overlays. Card eviction may still destroy and later restore an xterm
renderer, but it cannot destroy the surface's WebGL context.

## Invariants

1. A visible Agents Overview surface owns at most one live WebGL context for
   all of its terminal cards.
2. Scrolling, maximize/restore, card mode changes, and renderer residency never
   create another WebGL context for that surface.
3. A card renderer can be created or destroyed without losing the shared
   context or disturbing another card's pixels.
4. Draws are clipped to the registered terminal body and cannot overwrite
   another terminal or card chrome.
5. Terminal input, selection, links, mouse protocol, scrollback position,
   broker presentation state, and PTY geometry remain xterm/broker-owned.
6. Hidden Agents surfaces perform no draws and retain their context until the
   surface itself is destroyed.
7. Standalone Agent Session terminals retain their current renderer lifecycle.
8. WebGL unavailability produces one stable DOM fallback, not repeated context
   creation attempts.

## Implementation boundaries

Expected frontend changes are limited to:

- a shared context owner, CPU tile rasterizer, cell decoder, and draw pipeline
  under `src/features/terminal/`;
- an Agents-specific React context or explicit renderer-backend prop;
- `AgentTerminal` registration/invalidation hooks for shared mode while its DOM
  renderer remains mounted;
- the Agents Overview surface wrapper and viewport invalidation hooks;
- removal of per-card WebGL budget/demotion behavior only when shared mode is
  active;
- focused unit, browser, native, and performance instrumentation.

No Rust DTO, command, actor, replay, snapshot, provider filter, PTY, or geometry
change is expected. Any discovered need to change those systems stops the
implementation for a new design review.

## Failure modes and abort criteria

| Failure | Required response |
|---|---|
| A draw leaks into another card | Block shipping; fix viewport/scissor and coordinate mapping. |
| Cell attributes or custom glyphs differ materially from xterm WebGL | Block shipping; extend the public-buffer decoder or attributed glyph module before enabling shared mode. |
| Public xterm buffer APIs cannot represent a required visual state | Stop and review a maintained surface-renderer integration; do not use private xterm core hooks or fall back to per-card contexts. |
| Link, selection, input, or mouse behavior changes | Block shipping; preserve xterm's interaction layers or mirror only the missing visual decoration. |
| Context loss starts repeated recreation | Permanently select DOM fallback for that surface lifetime. |
| Shared mode requires broker or PTY changes | Stop and redesign; rendering must remain presentation-local. |
| Canvas dimensions exceed device limits | Keep the canvas viewport-sized; never allocate one backing canvas for total scroll height. |

## Verification

### Frontend unit and integration

- One Agents surface with many terminals calls the real-context factory once.
- Alternating terminals decode attributes independently and clip every draw.
- Default, 16-color, 256-color, RGB, style, wide, combined, custom-glyph,
  selection, cursor, and link-decoration fixtures match expected pixels.
- Resize and scroll recomposition uses deterministic card order.
- Card disposal removes only registration and cached draw data.
- Surface disposal releases the real context once.
- Context loss changes every resident card to DOM fallback without retry churn.
- Standalone `AgentTerminal` still uses the dedicated WebGL budget path.
- Agents maximize/restore and terminal/chat switching preserve presentation and
  xterm identity where the residency policy already promises it.

### Browser E2E

Browser E2E can prove canvas cardinality, card clipping, layout changes, focus,
selection, links, wheel ownership, and DOM fallback using deterministic mock
agents. Instrumentation must report real contexts and shared presentation
registrations separately.

### Native E2E

Native E2E is required for the rendering claim. Add an Agents-specific test
that exercises at least 16 active mock-provider terminals through:

- initial grid paint;
- repeated full-viewport scrolling;
- maximize and restore;
- Agents tab hide and reveal;
- terminal/chat/terminal switching;
- sustained output while cards enter and leave the viewport;
- wheel scrollback and terminal input after each transition.

The test must assert one real Agents WebGL context throughout, no context-loss
placeholder, no blank or cross-contaminated cards, stable scrollback evidence,
and unchanged PTY geometry for non-owning presentations.

Provider-specific fidelity claims require the existing opt-in real-provider
native rendering audit. At minimum, capture representative Codex, Claude, and
OpenCode states before calling their TUI rendering unchanged.

### Performance acceptance

Update the workbench performance audit so its terminal metrics distinguish:

- real browser WebGL contexts;
- shared Agents terminal presentations;
- resident xterm instances.

For one visible Agents Overview surface, the real terminal-context count must
stay at one while card count, scroll position, and maximize state change. Graph
and other non-terminal contexts remain separate and are reported independently.

## Documentation and evidence

Update `docs/developer/terminal-presentation-broker.md` to describe the
surface-owned Agents renderer and revised context budget. Because this changes
frontend rendering behavior, capture a feature-specific screenshot showing a
populated Agents grid after repeated scrolling and embed an uploaded HTTPS
image in the PR description.

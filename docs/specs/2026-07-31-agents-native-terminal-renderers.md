# Agents Native Terminal Renderers

**Status:** Implemented
**Date:** 2026-07-31
**Issue:** #791

## Decision

Agents Overview keeps every terminal-mode card xterm mounted and uses xterm's
native non-WebGL renderer. Agents cards do not load `@xterm/addon-webgl`, do not
participate in the process xterm/WebGL LRUs, and do not use a Wardian-owned
terminal compositor. Standalone Agent Session surfaces retain their existing
dedicated xterm WebGL lifecycle.

## Context

The original failure came from one WebGL context per Agents card. Chromium's
context cap caused older contexts to be lost as more cards were admitted.
Renderer recreation then destabilized scrollback and navigation.

A Wardian-owned shared WebGL compositor was prototyped because xterm does not
expose a supported API for sharing one WebGL context across terminals. That
prototype read xterm buffers and rasterized cells independently. It prevented
context churn but could not preserve xterm's complete rendering contract:
custom glyphs and graphical components split, link decoration and activation
regressed, text metrics diverged at scrollbar boundaries, and native scrollbar
behavior was replaced by platform-specific artifacts.

Reimplementing xterm rendering is outside Wardian's terminal lifecycle scope.
Correct terminal semantics take precedence over WebGL acceleration in the
multi-card surface.

## Runtime contract

- Every visible Agents terminal card has one persistent xterm instance.
- Agents xterms stay mounted across grid scrolling, maximize/restore, pane
  zoom, and temporary surface hiding.
- Agents xterms use xterm's native renderer and never load `WebglAddon`.
- Agents xterms bypass the process xterm LRU because adding a card must not
  evict another Agents card.
- Agents width transitions enable cursor-line reflow so the active line is not
  truncated.
- Standalone sessions keep their existing WebGL promotion, demotion, context
  budget, snapshot overlay, and renderer restoration behavior.

## Fidelity requirements

Xterm remains authoritative for:

- custom glyph and box-drawing rendering;
- wide, combined, and Unicode characters;
- ANSI colors, contrast, font metrics, cursor, and selection;
- link detection, hover decoration, and activation;
- native scrollbar geometry and interaction;
- accessibility and IME behavior.

Wardian must not place a visual terminal renderer over xterm in Agents
Overview.

## Verification

The production workbench audit seeds 32 Agents terminals and verifies that:

- all 32 xterms are mounted simultaneously;
- Agents Overview owns zero WebGL contexts;
- no Wardian shared-compositor canvas exists;
- xterm identities survive scrolling, resize, maximize/restore, tab changes,
  and pane zoom;
- complete terminal text survives those transitions;
- standalone terminal WebGL behavior remains covered by its existing tests.

Native runtime acceptance additionally checks real wheel scrollback with at
least two Agents xterms and captures xterm-native rendering evidence.

## Rejected alternative

A custom shared WebGL terminal compositor is rejected. A correct version would
need to reproduce or depend on unsupported xterm renderer internals, making it
fragile across xterm upgrades and likely to regress terminal fidelity again.

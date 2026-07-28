# Terminal scrollback ownership

## Status

Implemented with broker, frontend, browser, and native regression coverage.

## Problem

Wardian had three different interpretations of the same PTY stream:

- desktop xterm ignored Codex `CSI 3 J` through a frontend-only rewrite;
- the backend VT parser honored it and therefore published snapshots with the
  history erased;
- remote xterm received it unmodified and erased its own history.

This made a resident desktop terminal appear healthy until an authoritative
snapshot replaced it, while a newly attached mobile terminal could begin with
only the current frame. A foreground-only exception attempted to retain the
resident desktop buffer, but it left the canonical snapshot incomplete and
could not help mobile or a new presentation.

Mobile also intercepted every wheel and touch gesture in a capture listener.
That works for normal-buffer xterm history. In an alternate buffer with mouse
tracking, however, xterm has no normal scrollback to move: the terminal
application owns the wheel protocol. Wardian swallowed those events before
xterm could forward them to the application.

## Decision

The broker applies provider output policy before both VT parsing and event
publication. For Codex, `CSI 3 J` is removed by a chunk-boundary-safe filter.
Snapshots, desktop, and remote presentations therefore consume the same
canonical bytes. Frontend renderers do not independently rewrite scrollback
erase or infer full-screen clears from newline counts.

Snapshots are always authoritative. Initial registration, foreground recovery,
replay-gap recovery, activation, and generation changes use the same reset and
restore path. There is no resident-renderer exception.

Gesture ownership follows terminal protocol state:

- normal buffer: wheel and touch move xterm scrollback;
- alternate buffer without mouse tracking: xterm retains its native behavior;
- alternate buffer with mouse tracking: xterm forwards wheel input to the
  application, and remote touch travel is converted to the same wheel event.

The dormant scratch-terminal repaint path, direct xterm-buffer mutation,
synthetic-history insertion, and fuzzy duplicate deletion are removed. Provider
output is written through xterm's public VT parser. Native repaint duplicates
are preferable to heuristics that can delete real rows.

## Invariants

1. Canonical snapshots and published output events are derived from identical
   bytes.
2. A snapshot replaces local parser and renderer state before later events are
   applied.
3. Wardian never intercepts application-owned alternate-screen mouse input.
4. Gesture routing does not fit a renderer, resize a PTY, report a viewport, or
   change presentation ownership.
5. Canonical columns and rows remain owned by the terminal presentation broker;
   this change does not alter geometry calculation or rendering scale.

## Verification

- Rust coverage fragments `CSI 3 J` across PTY reads and proves both broker
  scrollback and published event bytes retain history.
- Frontend coverage proves output is not rewritten differently after the
  broker and snapshots still use the authoritative reset path.
- Remote unit coverage proves normal-buffer gestures move xterm while
  alternate-screen mouse gestures bypass xterm scrollback.
- Browser E2E uses real xterm state to prove mobile touch scrolling in both
  ownership modes.
- Native terminal wheel, background recovery, and geometry sweeps protect PTY,
  renderer, and layout behavior.

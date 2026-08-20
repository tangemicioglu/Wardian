# Window Restore Performance

* **Status:** Implemented
* **Date:** 2026-08-19

## Context

Live profiling of the desktop app found a visible subsecond stall after
minimize/restore while the native process remained responsive. The restore
path was producing overlapping native resize notifications, outer-window
fallback updates, terminal foreground work, and title-bar maximize checks.

## Decision

Treat native window dimensions as an event stream that is applied immediately
to CSS but published to layout listeners once per animation frame. A valid
native payload suppresses the outer-window fallback while that native stream
is settling.

Terminal foreground resynchronization is deferred until the first animation
frame after the document becomes visible. This lets the restored layout settle
before snapshot parsing and renderer updates begin. Title-bar maximize-state
queries use the same frame coalescing rule.

## Verification

The behavior is covered by frontend tests for resize coalescing and deferred
visibility recovery. The full frontend suite and a fresh native minimize/restore
smoke test pass.

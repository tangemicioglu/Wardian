# Terminal foreground recovery hardening

## Status

Implemented on the terminal foreground-resume path. Focused frontend and
terminal-session backend tests pass.

## Problem

Application foregrounding can invalidate a terminal snapshot while its resume
operation is still in flight. The previous queue de-duplicated by client, so a
rapid hide/show transition could discard the replacement request and leave a
visible terminal paused. Foregrounding also gave lower-priority visible panes
the same immediate scheduling as the interactive owner.

## Invariants

1. A foreground snapshot remains the authoritative cursor barrier. A client
   never acknowledges or drains events from an invalidated visibility epoch.
2. If visibility changes while a client is queued, the latest visible epoch
   requeues that client after the older operation finishes.
3. The interactive owner resumes first. Other visible mounted presentations are
   deferred to an idle callback, with a timer fallback for runtimes without
   `requestIdleCallback`.
4. Snapshot reuse is valid only while the terminal parser state and geometry
   are unchanged. Output and geometry changes invalidate the cached snapshot.
5. Timing diagnostics are debug-only and emit only slow foreground resumes;
   release builds do not add normal-path logging.

## Design

The frontend foreground queue records the visibility epoch associated with each
queued client. A requeue marker preserves a newer request when an older queue
item is still running. The queue records snapshot IPC, snapshot application,
acknowledgement, total resume, and queue-wait durations when terminal debugging
is enabled.

The Rust terminal actor retains the last completed snapshot for the current
parser state. Repeated snapshot requests return a clone of that immutable
value. The cache is cleared before processing parser-changing output and after
committing a geometry change.

## Non-goals

- Do not pause native PTYs or provider processes during application backgrounding.
- Do not change the snapshot DTO, replay limits, terminal ownership, or resize
  semantics.
- Do not infer off-screen state from focus or blur events.
- Do not hide a mounted terminal from the user without a presentation-level
  visibility state describing that decision.

## Verification

- Frontend TypeScript lint passes.
- Terminal session client and application-visibility tests pass, including a
  hide/show race during an in-flight snapshot.
- All terminal-session Rust tests pass, including snapshot cache invalidation
  after output.

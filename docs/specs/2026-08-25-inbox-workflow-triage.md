# Inbox workflow triage persistence

## Status

Accepted for issue #978.

## Problem

Workflow completion cards are projections of durable workflow-run checkpoints.
The Inbox also stores local read state in `queue/items.json`. Removing a read
workflow card from that file is not sufficient: the next Inbox refresh sees the
terminal workflow run and projects a new unread card. A refresh already in
flight can also replace a local read mutation with an older unread snapshot.

## Decision

- Keep workflow completion read state in the existing queue item.
- When a workflow completion is cleared or individually dismissed, persist a
  hidden triage marker containing its workflow and run identities.
- Treat the marker as evidence that the terminal run has already been
  projected, but exclude the marker from desktop and remote Inbox projections.
- Invalidate an Inbox load that started before a local queue mutation, and wait
  for queued local persistence before starting a new load.

## Invariants

1. Marking a workflow completion read survives polling and application reload.
2. Clearing a read workflow completion hides its card without deleting the
   workflow run, checkpoint, or event history.
3. A repeated terminal workflow event cannot recreate a dismissed card.
4. Pending approvals and unresolved provider-choice recovery remain protected
   from bulk triage.
5. Remote Inbox projection ignores local workflow triage markers.

## Verification

The store regression tests cover read persistence, clear-and-reload behavior,
and a stale refresh racing a local read mutation. Remote projection coverage
keeps dismissed workflow markers out of the shared Inbox and preserves them
through Clear read.

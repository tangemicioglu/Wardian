# Chat model selection save serialization

## Status

Implemented for the chat composer model and effort controls.

## Problem

Chat model selection persists agent configuration and then sends the provider's
live model command. The selector previously stayed enabled while those async
operations were active. A second selection could therefore start before the
first completed, allowing an older response to overwrite and live-apply after
the user's newer choice.

## Decision

Treat persistence plus live application as one serialized selection operation.
While it is active, disable both model and effort controls. Re-enable both only
after the operation succeeds or the existing error and rollback path finishes.
Show a compact saving status beside the controls while the boundary is active.

This boundary is intentionally local to chat. Spawn and Configure Agent retain
their existing selection behavior because they do not apply overlapping live
commands from this surface.

## Invariants

1. At most one chat model-selection mutation is active for an agent surface.
2. Model and effort inputs share the same mutation boundary.
3. A failed persistence operation still rolls back to the last persisted
   selection.
4. A successful persistence followed by a failed live command remains saved
   and reports the existing partial-success error.

## Verification

The chat regression test holds the first persistence request open, verifies
both controls are unavailable, and proves a second model choice cannot dispatch
until persistence and live application finish.

# Chat model selection save serialization

## Status

Implemented for the chat composer model and effort controls.

## Problem

Chat model selection persists agent configuration and then applies it to a live
provider. The selector previously stayed enabled while those async operations
were active. A second selection could therefore start before the first
completed, allowing an older response to overwrite and live-apply after the
user's newer choice.

Codex also does not accept the selected model and effort as parameters to
`/model`. Sending `/model <model>` enters the text as a normal prompt and can
produce a plausible assistant acknowledgement without changing the active
model. Codex requires an interactive sequence: submit `/model`, choose a model,
choose a reasoning level, and, for Max or Ultra, choose from the advanced
reasoning submenu.

## Decision

Treat persistence plus live application as one serialized selection operation.
While it is active, disable both model and effort controls. Re-enable both only
after the operation succeeds or the existing error and rollback path finishes.
Show a compact saving status beside the controls while the boundary is active.

For a live Codex agent, the backend owns the entire provider interaction. It
opens `/model`, reads canonical PTY snapshots to identify each picker, moves
from the current option to the requested option with arrow keys, and waits for
Codex's confirmation. The requested defaults are resolved from Codex's cached
provider-owned catalog; Wardian does not hard-code model order or availability.
The per-agent delivery lock covers persistence and every picker step so another
Wardian delivery cannot interleave with the sequence.

Persistence remains authoritative if live application fails. An off agent
reports the choice as saved for its next start or restart. A live timeout or
unexpected picker reports partial success and sends Escape to unwind any open
picker. Other providers retain their existing live command path.

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
5. Codex live application is complete only after both model and effort are
   confirmed by the interactive provider UI.
6. Provider defaults expand through the live Codex catalog, never a Wardian
   model list or assumed picker index.

## Verification

The chat regression test holds the first update request open, verifies both
controls are unavailable, and proves a second model choice cannot dispatch
until persistence and live application finish. Rust tests simulate the Codex
PTY and cover exact option parsing, default resolution, and the nested Ultra
sequence. The opt-in native provider test spawns real Codex, changes model and
effort through the command, and verifies the confirmed terminal footer.

# Terminal history checkpoints across desktop restarts

## Status

Implemented for desktop presentation recovery when no live terminal broker is
available.

## Problem

On Windows, closing or restarting Wardian destroys its in-memory terminal
broker and the ConPTY handles it owns. A provider process can remain alive, but
a new Wardian process cannot reattach to that old ConPTY. The restored agent is
therefore visible and live at the provider layer while a new terminal
presentation receives `SessionNotFound` and has no broker snapshot or
scrollback to render.

Provider telemetry is not a terminal checkpoint: it does not preserve exact VT
state, geometry, or the canonical output policy used by the renderer. Replaying
it would create another source of terminal truth.

## Decision

The desktop xterm headless parser serializes its already-normalized
presentation state after live broker output or a live broker snapshot. The
frontend debounces writes and selects the largest serializable history between
1,000 and zero scrollback rows that fits its 750 kB input budget. The backend
accepts a bounded 1 MB payload and stores one current plus one previous JSON
generation under `<WARDIAN_HOME>/terminal-checkpoints/`.

On `SessionNotFound` only, the frontend loads that checkpoint and writes it
directly to the parser and renderer. It does not pass the serialized xterm
state through provider output filters a second time. Whenever a broker is live,
the broker snapshot remains authoritative and the checkpoint is never loaded.

## Invariants

1. A checkpoint is a recovery cache, not a PTY attach or alternate broker.
2. Live broker snapshots always replace local terminal state.
3. The checkpoint uses the same normalized xterm parser state the desktop
   rendered, rather than agent watch output or provider transcripts.
4. Checkpoints are bounded, session-ID validated, local to `WARDIAN_HOME`, and
   removed when an agent is cleared or deleted.
5. A checkpoint cannot recover history that was never captured before an app
   restart, and cannot restore input to an orphaned ConPTY.

## Privacy and retention

The serialized terminal state may contain the same sensitive content that was
visible in the terminal. It remains local to the Wardian home, is not sent to a
remote client, is capped at one current and one previous generation per agent,
and is removed on clear or delete. Users who remove the Wardian home remove
these recovery caches as well.

## Verification

- Rust tests cover strict request decoding, bounded persistence, invalid session
  IDs, and fallback to the previous checkpoint if a replacement is interrupted.
- Frontend tests prove a live broker never loads the cache, `SessionNotFound`
  restores the serialized state into both xterm instances, and live broker
  output creates a debounced checkpoint.
- Native Tauri command coverage proves the registered commands can persist and
  load a checkpoint in an isolated `WARDIAN_HOME`; it does not claim to
  reattach an old Windows ConPTY.

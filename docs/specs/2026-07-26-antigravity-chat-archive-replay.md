# Antigravity Chat Archive Replay

## Status

Implemented.

## Problem

The Chat view loaded only the active provider log and in-memory watch state. Both
sources are bounded and can disappear when an Antigravity conversation has not
yet produced a usable transcript path, when the CLI rotates a transcript, or
after Wardian restarts. Wardian had already persisted delivered prompts and
captured provider events in each agent-owned conversation archive, but the
Chat view did not read that durable record.

## Decision

The backend now replays the agent-owned archive event streams whenever it loads
a chat transcript. Live provider and watch events still refresh the current
turn; archived rows are merged first and are tagged with their archive
conversation ID so identical prompts from separate conversations remain
distinct.

Provider session identity remains fail-closed. This does not infer an
Antigravity `--conversation` value from filesystem recency or a different
agent's cache entry.

## Consequences

- Previously captured chat rows survive app restarts, log rotation, and a
  temporarily missing provider transcript.
- A provider turn that never reaches Antigravity's transcript cannot be
  reconstructed as an assistant response, but Wardian's delivered prompt
  remains visible as durable evidence.
- The change applies to every provider because archive replay is independent
  of the provider-specific log parser.

# Phase 1 — Memory foundation

## Outcome

Wardian owns a separate `<WARDIAN_HOME>/memory.db` SQLite database that can be read and written consistently by the desktop app, CLI, and automations. Records are agent-owned, revisioned, inspectable, and independently evidenced.

## Data model

`memory_records` stores immutable revisions. `memory_id` identifies a logical memory and `revision` orders its history. Active revisions are selected by `status = active`; previous revisions become `superseded`, and a removal appends an audit event before marking the active revision `removed`.

Required fields: revision ID, memory ID, revision number, agent ID, optional normalized workspace, kind (`stable` or `current`), normalized text, evidence excerpt, SHA-256 evidence hash, status, predecessor/successor revision IDs, created/updated/verified timestamps, and optional idempotency key.

`memory_sources` stores zero or more source references for a revision. A source can identify a conversation range, artifact, direct observation, or automation run. One source may be primary. Links are for deep inspection only: deleting a linked conversation does not delete its memory revision, and deleting memory does not alter conversation retention.

`memory_events` is the audit and UI projection stream for save, update, remove, load, and consolidation actions. `memory_injections` stores the exact compiled context, ordered revision IDs, fingerprint, provider process key, and timestamp. `memory_consolidation_cursors` stores incremental archive progress.

All connections use WAL, foreign keys, a busy timeout, schema migrations, and one transaction per lifecycle operation.

## API and CLI

The Rust core exposes typed `save`, `update`, `remove`, `get`, `list`, `history`, and `recall` operations. Validation rejects empty normalized text/evidence, invalid scopes, missing agents, workspace records without a workspace, evidence hash mismatches, and conflicting idempotency keys.

`wardian memory` exposes the same operations directly against SQLite. `WARDIAN_SESSION_ID` supplies the default agent; the active agent workspace supplies the default scope. The default save scope is workspace. `--scope agent` is explicit. Output is structured JSON by default following existing CLI conventions.

The bundled Wardian CLI skill teaches agents to save clear preferences, decisions, corrections, lessons, and explicit “remember this” requests. It tells agents not to save ambiguous or transient chatter and never to claim `Memory saved` unless the API acknowledges the write.

## Recall selection

Baseline recall is deterministic and local. It selects active agent-wide records plus active records for the normalized active workspace. Stable records precede current state. Current state is ordered by verification recency. No model call, embedding service, or conversation archive is required.

## Tests

- Migration and WAL creation in an isolated Wardian home.
- Save/list/get/history/update/remove lifecycle.
- Agent and workspace isolation.
- Evidence hash and source-link persistence after the source file is absent.
- Stable/current ordering and stale labeling.
- Idempotent save/update retry.
- CLI default scope and explicit agent-wide scope.
- Concurrent CLI/core reads and writes without database-lock failures.

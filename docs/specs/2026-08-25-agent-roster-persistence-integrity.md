# Spec: Agent Roster Persistence Integrity

## Problem

The live agent map and `agent_order` are maintained separately, but roster
snapshots are built by iterating only `agent_order`. A partial or stale order
can therefore persist a `state.json` that silently omits live agents. Startup
restores from that projection and does not recover omitted entries from the
historical SQLite registry, leaving a surviving habitat unreachable from the
roster.

## Decision

Treat the live agent map as the complete membership set for every roster
snapshot. Preserve the caller's valid order, then append any live agents that
are absent from the order in deterministic session-ID order. This makes
internal lifecycle snapshots lossless even if an order is stale.

The public `reorder_agents` command additionally requires an exact permutation
of the live agent IDs. It rejects missing, unknown, and duplicate IDs before
mutating the order or writing `state.json`.

This change does not resurrect every historical SQLite row into the active
roster. Deletion and retirement semantics remain distinct from snapshot
integrity; the fix prevents an active in-memory agent from being dropped by a
truncated order.

## Invariants

1. Every live agent appears exactly once in a successful reorder request.
2. Every roster snapshot contains every live agent exactly once.
3. Invalid reorder requests do not mutate `agent_order` or persisted state.
4. Missing IDs repaired during an internal snapshot are appended
   deterministically and recorded in the debug log.

## Verification

- Unit coverage validates complete, incomplete, unknown, and duplicate orders.
- A persistence regression verifies that a snapshot with a missing order entry
  still writes every live agent to `settings/state.json`.
- Lifecycle and restart coverage should continue to exercise pause, clear,
  resume, and restore paths because they share the snapshot helper.

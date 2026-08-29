# Phase 3 — Optional consolidation automation

## Outcome

Users may opt into automated extraction and consolidation without introducing a privileged memory subsystem or hidden quota usage.

## Normal automation behavior

Wardian bundles a `memory-consolidation` sample using the same seeding rules as every bundled automation: seed only when missing, preserve user edits, never auto-run, and provide no memory-specific reset, fork, compare, restore, or overwrite buttons.

The sample contains a normal task node with strict structured output followed by an explicit `memory_commit` node. The task instructs the selected curator to emit candidate saves, updates, supersessions, removals, and a durable source cursor. The user chooses provider, model, effort, workspace, and role assignment through normal automation configuration. No fallback provider or model is selected if that invocation fails.

## Memory commit node

`memory_commit` is a first-class engine node and the sole automation mutation boundary. It reads structured output from a named upstream node, validates the complete batch, and commits memory revisions, sources, cursor movement, and audit events in one SQLite transaction. Every batch requires an idempotency key derived from automation run and source cursor. Replaying the key returns the original result without new revisions.

Ordinary task, script, shell, state, and notification nodes cannot mutate memory implicitly.

## Generic session-close invoker

A persisted session-close invoker binds any blueprint to an agent conversation boundary. It contains an ID, blueprint ID, name, enabled state, source-agent filter, provider/workspace/input/bindings/assignments, and optional boundary-reason filters. It uses the same blueprint resolver, run artifacts, live executor, inbox projection, and assignment normalization as manual and scheduled invocations.

After conversation rollover/discard completes, Wardian submits an invocation payload containing agent ID, agent name, workspace, provider, conversation ID when available, boundary reason, and archive availability. The lifecycle operation never waits for the curator run. Failed launches are recorded as failed automation runs and do not roll back the agent clear/close.

The consolidator reads only archive ranges after its durable per-agent/workspace cursor. If conversation logging is disabled or no new range exists, it produces a no-op result. Memory itself remains available regardless of archive settings.

## Tests

- Bundled sample parses, validates, and preserves a user-edited copy.
- Disabled invokers do not run; enabled matching invokers launch exactly once.
- Nonmatching agent and boundary filters do not run.
- Strict output rejection performs no partial writes or cursor movement.
- Retried `memory_commit` returns the prior result without duplicates.
- Cursor advancement and memory changes are atomic.
- Conversation logging disabled produces a safe no-op.
- Existing manual and scheduled automation behavior remains unchanged.

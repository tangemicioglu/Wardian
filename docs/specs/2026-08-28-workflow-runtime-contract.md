# Workflow Runtime Contract

- **Status:** Implemented in this change
- **Date:** 2026-08-28
- **Issues:** [#1010](https://github.com/wardian-app/Wardian/issues/1010), [#1011](https://github.com/wardian-app/Wardian/issues/1011)

## Decision

Workflow authoring and execution must share an explicit runtime contract. A
node may be present in the registry for taxonomy and forward compatibility, but
it must not be presented as executable unless validation and the durable engine
can execute it.

## Branch conditions

Version 1 conditions are truthiness checks against one registry path. A path is
made of dot-separated segments. Each segment starts with an ASCII letter or
underscore and may continue with ASCII letters, digits, underscores, or
hyphens. Examples:

```text
nodes.agent-1.output.ready
trigger.output.approved
storage.release_channel
```

Boolean values use their value; `null`, zero, and empty strings are false;
non-empty strings and objects/arrays are true. Missing paths are false.

Operators, comparisons, literals, function calls, and compound expressions are
not part of this grammar. Validation returns the stable `invalid_condition`
diagnostic instead of allowing an expression to become a misleading false
branch. The engine repeats the check at its direct runtime boundary so callers
that bypass the command validation cannot silently route to `on_false`.

The same path grammar applies to a loop's optional `until` field. Bounded
comparisons can be added as a versioned language extension later; they must not
be inferred from the current text field.

Decision choices are output-port identifiers. They must be non-empty, unique,
and use the same ASCII identifier shape as other workflow ports (a letter or
underscore followed by letters, digits, underscores, or hyphens). Every
declared choice must have an outgoing edge, and every edge must name a declared
source port and a declared target input port. This keeps dynamic React Flow
handles and runtime routing in agreement before a blueprint is saved or run.

## Sub-workflows

`sub_workflow` remains in the registry as a reserved taxonomy entry, but is
marked `supported: false`. Validation reports `unsupported_node_type`, the CLI
labels it as unsupported, generated reference documentation records its status,
and the Builder does not offer it in the node library.

This is an intentional safety boundary. Executing a child workflow requires a
separate durable child run, typed input mapping, assignment and workspace
inheritance, parent/child provenance, result mapping, cancellation and failure
propagation, approval parking and restart, recursion limits, and a concurrency
policy. Until those semantics are designed and implemented together, a
blueprint cannot claim that the node is runnable.

## Durable state and cancellation

State nodes are deterministic engine operations. `set` and `merge` update the
run registry's `storage` object, `delete` removes named keys, and `get` returns
the requested keys or the complete storage object when no keys are supplied.
Mutations are represented by `state_updated` events, so checkpoints and replay
produce the same storage state. This storage is scoped to one run; it is not a
cross-run or cross-agent shared store.

`workflow_cancel` writes a durable marker. The engine consumes it before the
next dispatch boundary and records `run_failed` with
`workflow cancelled by operator`. If the run is parked for approval, the
command records that terminal event immediately. A provider call already in
progress finishes before this cooperative boundary is observed.

Checkpoints written before run storage was introduced are normalized on load by
adding an empty storage object, so resuming an older durable run uses the same
state shape as a new run.

Every new `run_started` event also carries its run id. If a process appends an
event but fails before writing its checkpoint, recovery folds that event log and
keeps the original run identity. Replay and resume share strict contiguous
sequence validation; a missing or reordered event is rejected by both paths.

Each new run also stores an immutable parsed blueprint snapshot and its
content hash. Resume, replay, and approval continuation reject a changed graph
or invalid blueprint rather than silently applying new routing to an old run.
Approval decisions must name the approval node recorded by the latest durable
`awaiting_approval` event. Cancellation of an approval-parked run uses its
checkpoint and event log directly, so it remains terminal even when the
mutable library blueprint is unavailable.

Loop containers must contain at least one body node. Empty loops are rejected
before startup, so the runtime never creates an active run with no executable
loop transition. Nested loops are also rejected until their replay semantics
can reset the complete nested body and loop state on every outer iteration.
Every parented body node must also be reachable from the loop's `body` port; a
parent annotation alone is not an execution edge.

Observe's node inspector limits output and error evidence to the selected
timeline position, so scrubbing cannot reveal events from the future.

Notifications continue to write the workflow run log and, for app-owned runs,
are now sent through `tauri-plugin-notification`. Headless/CLI execution keeps
the durable log behavior without requiring a desktop notification handle.

## Audit boundaries

This audit found two additional surfaces that must not be mistaken for
completed runtime functionality:

- Busy-agent `wait` and `queue` assignment policies still return explicit
  errors. `skip` and `fail` remain the only terminal busy-agent policies until
  a persisted wait/queue contract exists.
- The legacy `WorkflowLibrary` component and `useWorkflowLibrary` hook contain
  dead persistence calls and console-log context-menu actions. They are not
  mounted by the current `WorkflowsView`, which uses the on-disk blueprint and
  run commands. They remain a retirement or implementation follow-up, not an
  active workflow capability.

## Verification contract

The core tests cover both branch ports, unsupported sub-workflow validation,
expression rejection, state mutation and replay, and cancellation marker
consumption. Generated Builder schema and node-reference documentation must be
regenerated whenever the registry contract changes.

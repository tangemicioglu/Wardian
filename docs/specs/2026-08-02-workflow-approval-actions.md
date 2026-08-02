# Workflow approval actions

## Context

An approval gate parks a workflow until a person selects **Approve** or
**Reject** from Workflow Observe or Inbox. The previous approve command waited
for every later workflow step to finish before responding to either surface.
That left the run visibly awaiting approval and made an accepted decision look
like a failed button click. Inbox also projected workflow approvals without
rendering their choices.

## Decision

Approval has two durable phases:

1. Persist `approval_granted` synchronously, changing the run from
   `awaiting_approval` to `running`.
2. Continue the remaining workflow in a background task using that persisted
   state.

Reject remains synchronous because it only persists the terminal decision.
Both Observe and Inbox call the same command, pass an explicit null note, and
refresh from the durable run state after it succeeds. Workflow approval cards
in Inbox always expose their available **Approve** and **Reject** choices.

The workflow engine serializes approval decisions per run before it reads or
writes the checkpoint. A second simultaneous choice from another surface is
rejected without appending an event or starting another continuation.

The controls disable while their request is pending and present command
failures inline. A failed background continuation remains represented by the
workflow's persisted run events and state.

## Verification

- Core engine coverage proves an accepted gate is durably `running` before any
  continuation task executes.
- Frontend coverage proves Observe sends the durable decision and exposes
  backend errors.
- Inbox coverage proves a projected workflow gate offers and sends its approval
  action.
- Native coverage races duplicate and conflicting approval requests. It proves
  the run records one decision event, one continuation only when approved, and
  monotonic event sequence numbers.

# Cross-Agent Delivery and Watch Reliability

## Decision

Normal live messages use a durable mailbox whenever delivery must wait for a
safe provider input surface. A queued message survives an app restart, has one
retry worker per target, and is removed only after a terminal delivery outcome
is recorded. Retry is bounded to five minutes so a message cannot be injected
into a later, unrelated turn. `send --wait-until idle` waits for a
provider-confirmed completion of the exact delivered turn, not a generic Idle
observation.

## Failure Breakdown

1. A conditional `agent watch --until status:idle` could match a retained Idle
   event. The watch endpoint passed no cursor to the retained event snapshot,
   and the status matcher accepted any matching event. An agent could therefore
   still be processing after the command reported completion.

2. Busy-target messages existed only in process memory. Restarting the app
   discarded them. In addition, the retry path made one delayed attempt; if the
   input surface was still unavailable at that instant, no worker remained to
   retry. There was also no delivery-age bound. Conversation evidence showed
   messages queued hours before they were eventually submitted, sometimes
   after related work had already finished.

3. Message interactions stayed `queued` after transport succeeded. Delivery
   attempts recorded `submit_sent_unconfirmed` or `provider_applied`, but the
   interaction record did not advance, making durable state misleading during
   troubleshooting and recovery.

4. `send` is deliberately one-way, but callers sometimes treated a transient
   status change as a reply or completion receipt. The absence of a
   provider-confirmed turn-complete event in the control-plane watch stream
   made that misuse easier. OpenCode also classified a normal terminal
   `step_finish` as a generic model response instead of a completed turn.

## Design

- Store pending mailbox rows in `state.db` with their interaction ID, target,
  input mode, queue policy, origin, and dispatch phase.
- On startup, rehydrate pending work. If an in-flight row already has a submit
  receipt, mark it delivered without resending it. Requeue only work that has
  no terminal receipt.
- Claim one persistent retry worker per target. It retries every two seconds
  while pending work remains and exits atomically when the target has none.
  A queued message expires after five minutes rather than entering a later,
  unrelated provider turn; an agent that reports Ready but has no input channel
  also terminates the queued delivery instead of retrying indefinitely.
- Advance message interactions through `queued`, `delivering`, `delivered`, or
  `failed` from durable transport evidence. Structured ask task status remains
  independent of the message transport status.
- Emit `turn_completed` into the agent watch stream for provider completion
  events. All maintained providers must map their actual terminal completion to
  that event.
- Emit a message-specific `submit_started` watch event after its payload reaches
  the PTY and before the submit key. This is an ordering boundary: any provider
  response for that message occurs after it.
- For a normal live send, anchor status/output/event waits to that exact
  `submit_started` event. Translate `--wait-until idle` to
  `event:turn_completed`; keep headless delivery mapped to its synchronous
  `provider_applied` receipt.
- A conditional bare `agent watch --until ...` takes its cursor at invocation,
  while an explicit `--since` retains historical-query behavior.

## Non-Goals

`send` does not become a request/reply protocol; use `ask` for a durable,
structured reply. A caller that supplies an expired explicit watch cursor still
receives `gap_detected` rather than a guessed result, because the omitted
history cannot prove whether its condition occurred. Durable queueing is not
an unbounded eventual-delivery guarantee: messages that cannot reach a safe
input surface during the retry window are recorded as failed.

## Verification

Unit coverage proves mailbox persistence and recovery, retry-worker ownership,
interaction status progression, exact delivery anchors, fresh conditional watch
semantics, provider completion watch events, and OpenCode completion parsing.
Native end-to-end coverage exercises queued delivery through the app-owned PTY
runtime.

# Queue-if-busy Delivery

* **Status:** Implemented
* **Date:** 2026-08-21

## Context

An agent can start a provider turn before its status update reaches the
delivery router. Previously, a `queue-if-busy` message could therefore see an
idle snapshot, write directly to the live terminal, and time out waiting for a
new turn-start receipt. Retrying after that timeout is unsafe because the
payload may already be present in the provider composer.

## Decision

For a live agent that appears idle, `queue-if-busy` now records the message in
the durable mailbox before any terminal I/O. It then schedules one locked
mailbox drain. That drain reads the current provider readiness: an already
recorded busy turn leaves the record pending, while an actually ready target is
eligible immediately. It does not require a second idle status transition.
The fast path requires explicit ready evidence; an absent or unknown state
remains pending until Wardian observes a provider-ready event.

Mailbox dispatch takes the target delivery lock before selecting its next FIFO
record. It reserves the next provider turn before releasing that lock and
submitting the terminal input. Concurrent readiness notifications can therefore
submit at most one mailbox message to a target for a single ready turn.
The reservation also sets the target to processing when a telemetry drain has
no application handle, preventing a second telemetry pass from treating the
same turn as idle.

Existing routes that are already known busy, action-required, leased, or
headless keep their established queue behavior and release signals.

## Consequences

- **Positive:** A stale idle status cannot make `queue-if-busy` interrupt an
  in-progress provider turn.
- **Positive:** Parallel agents share a durable FIFO and one target input
  reservation rather than racing terminal writes.
- **Trade-off:** The first drain is still limited to the readiness state that
  Wardian has observed; subsequent provider-ready events remain the release
  mechanism for a target that is genuinely busy.

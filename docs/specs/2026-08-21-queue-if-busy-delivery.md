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
mailbox drain. After the initial mailbox row is durably written, Wardian
persists the current per-target provider-observation sequence as a per-record
readiness watermark. That drain reads the current provider readiness, but it
can only consume a Ready observation with a strictly later sequence. An already
recorded busy turn, a Ready observation that predates the durable write, or an
older Ready observation leaves the record pending. The sequence—not wall-clock
time—preserves the ordering even when two observations share a timestamp. The
fast path also requires explicit ready evidence; an absent or unknown state
remains pending until Wardian observes a provider-ready event.

Telemetry polling does not manufacture that event. A repeated cached `Idle`
sample may still update non-ready state, but it never advances the readiness
sequence or drains a mailbox. Only the provider-status observation path, which
carries a distinct status-observation sequence, or direct provider readiness
evidence can release an armed record.

Mailbox dispatch takes the target delivery lock before selecting its next FIFO
record. It reserves the next provider turn before releasing that lock and
submitting the terminal input. Concurrent readiness notifications can therefore
submit at most one mailbox message to a target for a single ready turn.
The reservation also sets the target to processing when a telemetry drain has
no application handle, preventing a second telemetry pass from treating the
same turn as idle.

Existing routes that are already known busy, action-required, leased, or
headless keep their established queue behavior and release signals.

On startup, a pending mailbox row written by a pre-watermark version is armed
under its target delivery lock using the restored provider-observation sequence.
Recovery rechecks the sequence after each durable arm and raises the watermark
until it covers any observation that raced the write. It therefore waits for
one new Ready observation after recovery instead of blocking FIFO delivery
forever or trusting pre-restart readiness.

## Consequences

- **Positive:** A stale idle status cannot make `queue-if-busy` interrupt an
  in-progress provider turn.
- **Positive:** Parallel agents share a durable FIFO and one target input
  reservation rather than racing terminal writes.
- **Trade-off:** A message sent to an apparently idle target waits for one
  causally newer Ready observation after its durable sequence watermark. This
  avoids treating the stale idle snapshot, readiness observed during a delayed
  mailbox upsert, or an equal-millisecond timestamp as permission to write
  terminal input.

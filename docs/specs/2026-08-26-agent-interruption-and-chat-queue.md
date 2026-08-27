# Agent Interruption and Chat Queueing

* **Status:** Implemented
* **Date:** 2026-08-26

## Context and Problem Statement

An interactive provider can continue emitting output briefly after a user
interrupts it. Wardian previously set the agent to `Idle` when Ctrl-C was
accepted, but a late provider event or telemetry pass could set that same
runtime back to `Processing`. Because mailbox delivery waits for an idle
observation, messages queued during this state could remain pending forever.

The desktop chat composer also changed its action to an interrupt button for
the entire processing state. Text entered before clicking the action was
therefore not sent through the existing queue-capable delivery path.

## Decision

1. Treat an accepted interrupt as a runtime-local status boundary. Ignore late
   busy or approval status events for that runtime until new input is delivered
   or the runtime is replaced.
2. Recognize explicit interrupted or failed Codex turns as idle for both live
   event parsing and session-log telemetry. A stopped turn must not leave the
   runtime in `Processing`.
3. Keep chat text editable while the provider is processing. If the composer
   contains text or attachments, its action submits through the normal
   `QueueIfBusy` path and is labeled `Queue message`. With an empty composer,
   the same action remains `Interrupt agent`.
4. Keep provider approval actions live-only. Only ordinary chat messages and
   commands use the queue-capable delivery behavior.

## Consequences

- **Positive**: Interrupts cannot be undone by stale provider events, and
  queued chat text remains durable until the next idle/ready delivery point.
- **Positive**: Desktop and remote chat continue to share one
  provider-agnostic delivery policy.
- **Positive**: The behavior is scoped to the exact runtime status handle, so
  a replacement runtime is not affected by an earlier interruption.
- **Negative**: A provider that emits no completion signal remains behind the
  interrupt boundary until the user submits new input or the runtime is
  replaced; that is preferable to falsely reporting active work.

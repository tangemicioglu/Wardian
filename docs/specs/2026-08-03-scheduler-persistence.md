# Scheduler Runtime Persistence

* **Status:** Accepted
* **Date:** 2026-08-03

## Decision

The workflow scheduler must persist `library/schedules.json` only when a tick
changes persisted runtime state. A tick that observes schedules but does not
advance, initialize, or remove one must not rewrite the schedule file.

Runtime changes include:

- computing a missing next-run timestamp;
- advancing an occurrence after a fire;
- changing pause or last-run fields; and
- removing an expired or one-time schedule.

The existing atomic temporary-file-and-rename write remains the persistence
mechanism whenever a change is detected.

## Rationale

The scheduler wakes on a fixed interval, so unconditional persistence caused a
steady stream of unnecessary writes while all schedules were idle. Conditional
persistence preserves the crash-safe file replacement behavior while removing
the idle write loop.

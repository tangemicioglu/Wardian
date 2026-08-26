# Telemetry and Roster Efficiency

## Problem

Wardian's desktop process can become unresponsive when a large restored roster
is active. Metrics sampling refreshes the entire operating-system process table
on every status tick, and the watchdog can detach a slow blocking sample before
starting another one. Restore events can also cause many independent roster
requests in the frontend.

## Design

- Metrics sampling remains on a five-second cadence for status, liveness, CPU,
  and memory updates.
- The process inventory and parent/child topology are rebuilt at most every 30
  seconds, or immediately when the set of agent process roots changes. Between
  rebuilds, only the last known agent trees are refreshed.
- Windows command-line and environment-marker discovery keeps its existing
  longer TTL. A new process is therefore discovered on the next inventory
  rebuild rather than on every status tick.
- Metrics ticks are single-flight. A slow blocking sample is awaited to
  completion before another sample can start; the watchdog reports latency but
  does not detach work that Tokio cannot cancel.
- `agents-updated` notifications are debounced for 100 ms and concurrent
  `list_agents` calls share one request. A mutation arriving during a request
  schedules at most one follow-up load so the final roster is not lost.

## Trade-offs

Process-tree membership and newly spawned descendants may be up to 30 seconds
old between inventory rebuilds. Existing known processes still receive
five-second resource and liveness sampling. A slow metrics pass can delay the
next pass, but it cannot multiply blocking workers and consume the process's
thread pool.

## Verification

- The telemetry unit test exercises a full inventory followed by a tracked-PID
  refresh and asserts that the inventory is reused and the tracked refresh is
  faster.
- The roster controller test emits a burst of lifecycle events and asserts
  that only one debounced reload occurs, while a concurrent-refresh test proves
  that one follow-up load is sufficient when state changes mid-request.
- Release-process measurements should compare `Wardian.exe` CPU usage and the
  `Slow telemetry pass` log before and after the change with the same active
  roster.

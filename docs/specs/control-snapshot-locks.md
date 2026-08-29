# Control Snapshot Locking

## Decision

The live-agent control snapshot loads persisted conversation leases before it
acquires any live-agent state lock. It then holds the global agent-map lock only
long enough to copy the `Arc`-owned fields needed for each snapshot. Config,
status, and timestamp guards are acquired after the map lock is released.

## Invariants

- Synchronous lease-file reads must not occur while `AppState::agents` or
  `AppState::agent_order` is locked.
- Snapshot construction must not retain the global agent-map lock while
  waiting on a per-agent field lock.
- The snapshot remains ordered by `agent_order`, with unlisted live agents
  appended as before.
- `ActiveAgent::config` is protected by a short, await-free synchronous guard;
  callers clone the value and release the guard before awaiting or doing
  blocking I/O.

## Verification

The control unit regression test holds a per-agent config guard and confirms the
global agent map can be acquired, proving the map lock is not retained during
per-agent reads. Backend tests and the repository's performance measurement
procedure remain the release gates for this change.

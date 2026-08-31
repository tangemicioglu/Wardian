# Remote PWA Roster Snapshot Availability

## Problem

The remote PWA cannot render its watchlist until the authenticated roster and
watchlist requests complete. The roster handler could wait on the global agent
map and order locks, or synchronously wait on a per-agent configuration or
status lock. Status persistence, provider restoration, or telemetry could make
that wait long enough to look like a failed remote load.

## Decision

`remote_agent_roster` is a non-blocking snapshot boundary. It uses `try_lock`
for the global agent/order maps and each live config/status pair. A complete
live roster is cached per `AppState` and returned while a later live read is
busy. Before a live snapshot exists, the endpoint reads the atomically written
`settings/state.json` configuration and marks active entries as `Restoring`.
The status stream remains authoritative and replaces provisional data as soon
as the runtime is available.

This keeps the remote PWA's startup contract responsive without changing
desktop agent lifecycle or remote action authorization. The already-running
desktop must still be rebuilt and restarted after this change so its embedded
gateway serves the repaired implementation.

## Verification

Regression tests hold the global agent lock and a per-agent configuration lock
while the roster is requested. They assert that the endpoint returns within a
bounded interval from the last complete snapshot. A first-load test holds a
live lock with no cache and verifies that the atomic persisted configuration is
returned instead. Existing roster field, ordering, and latest-text tests
remain green.

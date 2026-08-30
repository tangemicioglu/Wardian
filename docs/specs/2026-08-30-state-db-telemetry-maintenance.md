# State Database Telemetry Maintenance

## Decision

Normalize repeated telemetry strings into integer lookup references during the
schema v5 migration. Copying is resumable in 2,000-row transactions, and the
legacy fact tables remain available until counts are rechecked while the final
write lock is held. Compatibility views preserve the existing read-facing
columns and the insert triggers preserve older test and integration writers.

The migration and explicit maintenance command share an adjacent inter-process
lock. SQLite transactions still stay batch-sized, so the lock serializes
owners without holding a database write transaction for the entire copy.

## Retention and compaction

Retention is not automatic. An operator must provide the number of days and a
new backup destination with:

```bash
wardian telemetry maintain --retain-days <days> \
  --backup "<backup-path>/state.db.before-telemetry-maintenance" \
  --quiesced [--vacuum]
```

The operator must stop the desktop app and all agents first. The command creates
and integrity-checks the backup before mutating the source. It recomputes every
hourly bucket touched by old turns, edits, and completed activity intervals,
then deletes those raw rows in bounded batches and checkpoints the WAL. The
current rollup does not reproduce rate-limit history, so `telemetry_limits` is
retained. `VACUUM` is opt-in and runs only in this explicit offline path.

The research investigation's candidate window is 90 days, but it is not a
product default. Choosing a shorter or longer window remains an operator or
product decision until retention requirements are established by usage data.

## Evidence boundary

Hourly rollups answer aggregate dashboard queries but do not reproduce every raw
fact: distinct turns, file paths, activity intervals, or rate-limit history can
require detail rows. The maintenance path therefore deletes only fact families
whose historical aggregate can be rebuilt and keeps the non-reproducible limit
observations. The normalized lookup tables and source cursors remain so future
ingest continues to address the same provider sources.

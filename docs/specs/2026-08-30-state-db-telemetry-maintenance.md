# State Database Telemetry Maintenance

## Decision

Normalize repeated telemetry strings into integer lookup references during the
schema v5 migration. Copying is resumable in 2,000-row transactions, and the
legacy fact tables remain available until counts are rechecked while the final
write lock is held. Compatibility views preserve the existing read-facing
columns and the insert triggers preserve older test and integration writers.

The migration and core maintenance function share an adjacent inter-process
lock. During the v4-to-v5 copy, a file-backed database first leaves WAL for
SQLite's rollback journal, then the migration holds exclusive locking mode
across its batch transactions. Older binaries and offline writers must be
stopped before this transition: exclusive locking mode alone does not fence
writers while WAL is active. Each batch still commits its progress marker for
restart after interruption, and the connection restores its prior journal and
locking modes before the migration call completes.

The database keeps `schema_version=4` as the legacy write-ABI marker and records
`normalized_schema_version=5` separately. An older v4 client therefore does not
enter its destructive unknown-version reset path; its legacy index setup fails
closed against the compatibility views. Older clients must be upgraded before
using this database again.

## Retention and compaction

Retention and compaction are application-owned software operations. Application
code calls the core function with an explicit retention policy, backup
destination, and vacuum choice. There is no destructive telemetry
maintenance command in the CLI and no silently selected product default.

The software call is the normal core API:

```rust
wardian_core::telemetry::maintain(&connection, retain_days, backup_path, vacuum)?;
```

The caller owns policy selection and schedules this function only at a
quiescent lifecycle boundary; the function owns backup verification, durable
recovery markers, deletion, checkpointing, and optional compaction.

The function creates and integrity-checks the backup before mutating the
source. It rebuilds every
hourly bucket touched by the old turns, edits, and completed activity intervals
and records a durable prepared phase before deleting raw rows in bounded
batches. An interrupted retry resumes that phase without rebuilding rollups
from rows already pruned, so their historical contributions remain intact. The
persisted cutoff is canonical while that phase is in progress, so a retry after
the clock crosses an hour still resumes the same boundary; the retry must keep
the original retention window. It then checkpoints the WAL and clears the phase
marker. The
current rollup does not reproduce rate-limit history, so `telemetry_limits` is
retained. `VACUUM` is opt-in and runs only when the software maintenance policy
requests it at a quiescent lifecycle boundary.

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

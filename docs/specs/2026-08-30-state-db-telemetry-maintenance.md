# State Database Telemetry Maintenance

Status: core recovery and compaction design; the application policy and
schedule are defined by
[`2026-08-30-state-db-automatic-telemetry-retention.md`](./2026-08-30-state-db-automatic-telemetry-retention.md).

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

The caller owns policy selection and schedules this function through the
application database serialization boundary; the function owns backup
verification, durable recovery markers, deletion, checkpointing, and optional
compaction. The former provider-runtime quiescence rule is superseded by
[`2026-08-31-live-fleet-telemetry-maintenance.md`](./2026-08-31-live-fleet-telemetry-maintenance.md).

The function creates and integrity-checks the backup before mutating the
source. A new backup is built through a temporary sibling and atomically
promoted only after verification; an existing verified backup may be supplied
when resuming a prepared phase. It rebuilds every
hourly bucket touched by the old turns, edits, and completed activity intervals
and records a durable prepared phase before deleting raw rows in bounded
batches. An interrupted retry resumes that phase without rebuilding rollups
from rows already pruned, so their historical contributions remain intact. The
persisted cutoff is canonical while that phase is in progress, so a retry after
the clock crosses an hour still resumes the same boundary; the retry must keep
the original retention window. It then checkpoints the WAL and clears the phase
marker. The
Rate limits are account-level gauges. The write path keeps only the newest
observation per provider, and maintenance removes older observations already
present in an installed database. `VACUUM` is opt-in and runs only when the
software maintenance policy requests it through the serialized application
maintenance path.

The desktop application currently supplies a 90-day product policy. Choosing a
shorter or longer window remains a product decision and requires updating the
automatic-retention specification and its application constant together.

The v4-to-v5 source repair merges complete compatible alias state from the
furthest cursor. Incompatible parser, cursor, medium, or file-identity state
resets the canonical source and purges its facts for a single complete reread.
Turns, edits, and activity intervals all follow the same alias map. A versioned
rollup repair runs once for upgraded stores so existing buckets are reconciled
after alias deduplication without adding work to every startup.

## Evidence boundary

The Dashboard reads its trailing window on open, on a 15-second backstop, and
when telemetry changes. Analytics reads on a two-minute backstop and on the
same update event. Neither surface requests individual provider events. The
five-minute compact facts preserve the finest Analytics cells, distinct turn
and file counts, model attribution, and rollup repair for byte-offset sources;
timestamp-cursor sources retain event identities only because their overlap
reads require them for deduplication. Rate-limit history is intentionally not a
retained contract; the current gauge is sufficient. Source cursors and
fingerprints remain so future ingest continues to address the same sources.

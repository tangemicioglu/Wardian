# State Database Telemetry Review Corrections

Status: implemented on `fix/state-db-growth`.

## Decision

The v4-to-v5 telemetry migration treats source aliases as one physical source
only after their persisted state has been reconciled. Compatible aliases keep
the complete state from the furthest cursor, including cursor kind, parser
version, fingerprint, file position, and parser carry fields. If those state
identities are incompatible—including a mixed known/unknown fingerprint
pair—migration removes the affected facts and resets the canonical source so
the next ingest performs one complete reread.

Source ownership is applied consistently to turns, edits, and activity
intervals. Alias repair deduplicates intervals under the existing
`(session_id, started_at)` identity, and stale-source recovery can therefore
remove every interval produced by the canonical source.

Hourly rollups remain derived data. A one-time migration marker causes the
upgrade to rebuild every existing rollup bucket plus every bucket represented
by normalized facts or activity intervals. This repairs buckets inflated by
pre-migration aliases and avoids repeating the rebuild on ordinary startup.

## Recovery and retry contract

Telemetry maintenance writes a backup to a temporary sibling path, verifies
SQLite integrity, and atomically promotes it to the automatic backup name. Any
returned creation or verification failure removes the temporary output. The
application also removes abandoned temporary outputs left by a terminated
process before retrying.

The core first records a durable pending-attempt marker before creating the
backup. After verification, it records that the backup is associated with the
attempt. The scheduler uses one stable pending backup path for that attempt, so
failures before the prepared cutoff are retried against the same baseline
instead of creating one full database copy per retry. Before association, the
stable pending file is refreshed on each retry so newly ingested telemetry is
included; once the cutoff is
prepared, the pending baseline is reused until the phase completes. A legacy
prepared run first reuses a valid pending file left by an interrupted adoption;
otherwise the scheduler selects the newest verified automatic baseline that
predates the prepared-cutoff marker and moves it into that pending slot before
association. If no such baseline exists, it fails closed without rotating
automatic backups. Only then is the pending file promoted and normal two-file
rotation run. If a process ends after verifying the pending file but before
recording its durable association, the pending-attempt marker causes the
pending file to be refreshed at the same stable path on the next attempt;
invalid or missing pending files are replaced there. If no pending-attempt
marker exists, an
unassociated pending file is removed before a fresh attempt so it cannot omit
telemetry written after the prior attempt. This keeps pre-association retries
bounded while retaining a verified recovery baseline.
After association, a missing or corrupt pending file fails closed without
creating a replacement snapshot or rotating backups.

The CLI remains read-only. The desktop ingest coordinator owns the retention
opportunity and invokes the core operation through the serialized application
maintenance path after a source pass and an in-memory deadline. The former
provider-runtime quiescence condition is superseded by
[`2026-08-31-live-fleet-telemetry-maintenance.md`](./2026-08-31-live-fleet-telemetry-maintenance.md).

## Evidence

Focused schema and maintenance tests cover rollup reconciliation, furthest
cursor preservation, incomplete-fingerprint reset, replay suppression,
activity re-keying and stale-source cleanup, temporary-backup cleanup, retry
baseline association and reuse, and resumption after the last expired row has
already been deleted.

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

The application first records that its verified backup is associated with the
attempt. The scheduler uses one stable pending backup path for that attempt,
so failures before the prepared cutoff are retried against the same baseline
instead of creating one full database copy per retry. Once the cutoff is
prepared, the pending baseline is reused until the phase completes. A legacy
prepared run first reuses a valid pending file left by an interrupted adoption;
otherwise the scheduler selects the newest verified automatic baseline that
predates the prepared-cutoff marker and moves it into that pending slot before
association. If no such baseline exists, it fails closed without rotating
automatic backups. Only then is the pending file promoted and normal two-file
rotation run. For a fresh attempt, an unassociated pending file is removed
instead of reused because it may predate newly ingested telemetry.
After association, a missing or corrupt pending file fails closed without
creating a replacement snapshot or rotating backups.

The CLI remains read-only. The desktop scheduler remains the owner of the
retention policy and invokes the core operation only at a provider-runtime
quiescent boundary.

## Evidence

Focused schema and maintenance tests cover rollup reconciliation, furthest
cursor preservation, incomplete-fingerprint reset, replay suppression,
activity re-keying and stale-source cleanup, temporary-backup cleanup, retry
baseline association and reuse, and resumption after the last expired row has
already been deleted.

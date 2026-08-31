# State Database Telemetry Review Corrections

Status: implemented on `fix/state-db-growth`.

## Decision

The v4-to-v5 telemetry migration treats source aliases as one physical source
only after their persisted state has been reconciled. Compatible aliases keep
the complete state from the furthest cursor, including cursor kind, parser
version, fingerprint, file position, and parser carry fields. If those state
identities are incompatible, migration removes the affected facts and resets
the canonical source so the next ingest performs one complete reread.

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

When a retention phase has a prepared cutoff, the application selects the
newest verified automatic backup and reuses it as the recovery baseline. It
does not create another full database copy for each retry. The baseline remains
until the phase completes; only then does normal two-file rotation run.

The CLI remains read-only. The desktop scheduler remains the owner of the
retention policy and invokes the core operation only at a provider-runtime
quiescent boundary.

## Evidence

Focused schema and maintenance tests cover rollup reconciliation, furthest
cursor preservation, replay suppression, activity re-keying and stale-source
cleanup, temporary-backup cleanup, and reuse of a verified retry baseline.

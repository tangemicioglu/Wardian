# Live-Fleet Telemetry Maintenance

Status: implemented by PR #1109 for #1105.

## Context

The desktop previously ran a separate maintenance scheduler that waited until
every managed provider runtime was absent. Idle agents keep provider processes
alive, so a normal fleet never met that condition. This left the 90-day
raw-detail policy and verified backup creation unreachable during ordinary
operation. Rate-limit gauges are already bounded at ingest, and periodic SQLite
compaction is not part of this retention policy.

## Decision

The existing telemetry ingest coordinator owns the maintenance opportunity. It
retains one in-memory deadline and considers maintenance after a source pass.
When the deadline has not elapsed, the only added work is an `Instant`
comparison: no database query, backup, write, or additional timer task runs.
After a successful or no-op due check, the next opportunity is one day later;
after failure it is fifteen minutes later. The CLI remains read-only.

The application calls retention through the core database connection. Its global
mutex serializes the complete backup, retention, and WAL checkpoint sequence
with source ingest and normal application database work. The core telemetry
lease independently excludes a concurrent schema migration. A separate writer
that contends for SQLite work receives contention rather than interleaving with
the destructive pass; ingest logs the failure and retains the shorter retry
deadline.

The ingest-owned opportunity deliberately does not request periodic `VACUUM`.
The preceding source-alias migration was a one-time mass deletion; SQLite
reuses those free pages as future raw telemetry arrives. The core operation
retains its explicit opt-in compaction capability for a separately justified
operator policy.

This replaces the provider-runtime quiescence requirement in
`2026-08-30-state-db-automatic-telemetry-retention.md`. Provider process state
is not a database safety boundary.

## Consequences

On a normal running fleet, the first ingest pass after the deadline invokes the
cheap age check. Due telemetry then creates a verified recovery
baseline under `<wardian-home>/backups/telemetry`, removes aged raw facts in
resumable bounded batches, and checkpoints the WAL. Rate-limit gauges already
retain one current observation per provider during ingest; they are not a
recurring maintenance accumulation. Provider ingest waits briefly while the
pass owns the database; it resumes afterward without losing source cursors or
facts.

The pass can temporarily delay dashboard queries and rare independent SQLite
writers. That latency is accepted because it is bounded by SQLite contention
timeouts and the operation is daily and due-only. Deletion bounds live rows and
releases pages for SQLite reuse; it does not claim to shrink the operating-system
file without explicit compaction.

This design deliberately does not add a persisted scheduling heartbeat or a
second queue. Restarting the application resets the in-memory startup delay and
may perform one additional age check, but that is preferable to a new recurring
write protocol. Maintenance awaits completion before the coordinator schedules
the next ingest pass, so two app-owned telemetry writers never race for the
database mutex.

## Alternatives Rejected

- Startup or shutdown-only reclaim: requires a lifecycle interruption and does
  not satisfy reclamation while a live fleet operates normally.
- A separate daily scheduler: duplicates lifecycle ownership and can wake only
  to contend with the ingest loop for the same database mutex.
- Inline maintenance in each source transaction: would make a normal cursor
  advance pay for a global backup and couple source correctness to unrelated
  retention failures.
- A persisted maintenance heartbeat: avoids an age check after frequent restarts
  but adds a write and recovery contract to every installation for negligible
  steady-state value.
- Periodic VACUUM: the observed free pages arose from a one-time source-alias
  repair and are reusable by SQLite, so a daily rewrite is not warranted.
- A free-page size trigger: may be useful future policy, but does not repair
  the unreachable daily call path and is unnecessary to meet the current
  retention guarantee.
- Logging skipped quiescence: the scheduler no longer skips for provider
  liveness, so observability of that obsolete condition would not restore the
  product behavior.

## Verification

Focused maintenance tests seed expired turns, edits, and activity intervals,
then prove the application-owned due path creates a verified backup, removes
stale detail, checkpoints without VACUUM, and leaves the database ready for
normal reuse. Ingest scheduling coverage proves ordinary passes stop at the
in-memory deadline comparison. The application has one telemetry lifecycle loop
and never consults provider runtime state before retention.

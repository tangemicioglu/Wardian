# Live-Fleet Telemetry Maintenance

Status: implemented by #1105.

## Context

The desktop scheduler previously waited until every managed provider runtime
was absent before calling telemetry maintenance. Idle agents keep provider
processes alive, so a normal fleet never met that condition. This left the
90-day raw-detail policy and verified backup creation unreachable during
ordinary operation. Rate-limit gauges are already bounded at ingest, and
periodic SQLite compaction is not part of this retention policy.

## Decision

The desktop application must run its daily telemetry maintenance pass even when
provider runtimes are live. The scheduler remains the sole owner of the 90-day
policy and of automatic backup rotation; the CLI remains read-only.

The application calls retention through the core database connection. Its global
mutex serializes the complete backup, retention, and WAL checkpoint sequence
with provider ingest and normal application database work. The core telemetry
lease independently excludes a concurrent schema migration. A separate writer
that contends for SQLite work receives contention rather than interleaving with
the destructive pass; the scheduler logs the failure and retries after fifteen
minutes.

The scheduler deliberately does not request periodic `VACUUM`. The preceding
source-alias migration was a one-time mass deletion; SQLite reuses those free
pages as future raw telemetry arrives. The core operation retains its explicit
opt-in compaction capability for a separately justified operator policy.

This replaces the provider-runtime quiescence requirement in
`2026-08-30-state-db-automatic-telemetry-retention.md`. Provider process state
is not a database safety boundary.

## Consequences

On a normal running fleet, due telemetry now creates a verified recovery
baseline under `<wardian-home>/backups/telemetry`, removes aged raw facts in
resumable bounded batches, and checkpoints the WAL. Rate-limit gauges already
retain one current observation per provider during ingest; they are not a
recurring maintenance accumulation. Provider ingest waits briefly while the
pass owns the database; it resumes afterward without losing source cursors or
facts.

The pass can temporarily delay dashboard queries and rare independent SQLite
writers. That latency is accepted because it is bounded by SQLite contention
timeouts, the operation is daily and due-only, and it restores persistent disk
space without asking an operator to stop a fleet.

## Alternatives Rejected

- Startup or shutdown-only reclaim: requires a lifecycle interruption and does
  not satisfy reclamation while a live fleet operates normally.
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
normal reuse. The application startup call site invokes the scheduler without
consulting provider runtime state.

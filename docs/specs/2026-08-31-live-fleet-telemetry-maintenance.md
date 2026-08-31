# Live-Fleet Telemetry Maintenance

Status: implemented by #1105.

## Context

The desktop scheduler previously waited until every managed provider runtime
was absent before calling telemetry maintenance. Idle agents keep provider
processes alive, so a normal fleet never met that condition. This left the
90-day raw-detail policy, rate-limit gauge trimming, backup creation, and
SQLite compaction unreachable during ordinary operation.

## Decision

The desktop application must run its daily telemetry maintenance pass even when
provider runtimes are live. The scheduler remains the sole owner of the 90-day
policy and of automatic backup rotation; the CLI remains read-only.

The application calls maintenance through the core database connection. Its
global mutex serializes the entire backup, retention, WAL checkpoint, and
VACUUM sequence with provider ingest and normal application database work. The
core telemetry lease independently excludes a concurrent schema migration. A
separate writer that contends for SQLite's exclusive work receives SQLite
contention rather than interleaving with the destructive pass; the scheduler
logs the failure and retries after fifteen minutes.

This replaces the provider-runtime quiescence requirement in
`2026-08-30-state-db-automatic-telemetry-retention.md`. Provider process state
is not a database safety boundary.

## Consequences

On a normal running fleet, due telemetry now creates a verified recovery
baseline under `<wardian-home>/backups/telemetry`, removes raw facts older than
90 days in resumable bounded batches, keeps the newest rate-limit gauge per
provider, and vacuums released pages. Provider ingest waits briefly while the
pass owns the database; it resumes afterward without losing source cursors or
facts.

The pass can temporarily delay dashboard queries and rare independent SQLite
writers. That latency is accepted because it is bounded by SQLite contention
timeouts, the operation is daily and due-only, and it restores persistent disk
space without asking an operator to stop a fleet.

## Alternatives Rejected

- Startup or shutdown-only reclaim: requires a lifecycle interruption and does
  not satisfy reclamation while a live fleet operates normally.
- Separate retention from VACUUM: adds a new mixed-phase recovery protocol even
  though the existing app mutex already serializes both phases with ingest.
- A free-page size trigger: may be useful future policy, but does not repair
  the unreachable daily call path and is unnecessary to meet the current
  retention guarantee.
- Logging skipped quiescence: the scheduler no longer skips for provider
  liveness, so observability of that obsolete condition would not restore the
  product behavior.

## Verification

Focused maintenance tests seed expired turns, edits, activity intervals, and
duplicate provider gauges, then prove the application-owned due path creates a
verified backup, removes stale detail, retains the newest gauge, and compacts
the database. The application startup call site invokes the scheduler without
consulting provider runtime state.

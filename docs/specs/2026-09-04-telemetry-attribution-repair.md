# Telemetry Source Attribution Repair

## Decision

Telemetry facts are owned by the Wardian agent recorded on their canonical
provider source. A physical Codex or Claude transcript exposed through several
projected homes is one source, not one source per projection. Discovery and the
core ingest store therefore use the same provider-plus-physical-path identity.
The first claimant cannot overwrite a source already owned by another agent,
and parsed turns, edits, and activity intervals are rejected if they do not
match the source context.

Shared transcript paths that cannot be tied to a recorded provider session are
not attributed. Losing an ambiguous ingest pass is safer than copying the same
history into multiple agents; a later pass can retry after the provider session
is recorded.

## Historical recovery contract

`wardian telemetry repair` is an explicit, backup-aware recovery path. Its
`--db` argument identifies the database and `--backup` is mandatory with
`--apply`; a dry run opens the database read-only. Before any telemetry schema
migration or fact mutation, the command:

1. acquires the telemetry maintenance lease;
2. creates a verified SQLite backup with `VACUUM INTO`, or verifies and reuses
   the existing backup at that path; and
3. leaves the original backup intact as the retry baseline.

The repair then runs telemetry migrations, refuses to modify source-less facts,
and, in one SQLite transaction:

- changes foreign turn and edit fact session references to the owning source's
  session reference;
- removes a foreign activity span only when the owner already has a span at the
  same start time, otherwise changes its session id to the owner; and
- rebuilds rollups from the resulting canonical facts and existing rollup
  bucket keys.

The transaction is committed only after all source-owned rows are processed.
Post-commit inspection must show zero foreign and source-less facts. Repeating
the command preserves valid rows and produces no further fact changes.

## Operational boundary

Operators must stop Wardian, older app binaries, and offline writers using the
selected home before applying the repair. The command does not infer a default
database, mutate an app-owned database implicitly, or silently choose a backup
location. After a successful report, restart Wardian to resume normal ingest.

## Evidence

Focused core regressions cover shared-file ownership rejection, all three fact
tables, valid-row retention, duplicate activity removal, idempotent reruns, and
agent-level dashboard breakdown differences. The CLI exposes the same dry-run
and apply contract documented above.

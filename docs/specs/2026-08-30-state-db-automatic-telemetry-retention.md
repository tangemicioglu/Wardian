# Automatic State Database Telemetry Retention

Status: implemented on `fix/state-db-growth`.

## Cause

`state.db` grew through normal telemetry ingestion, not through an oversized
WAL or control-history leak. A live read-only census found:

| Observation | Value |
| --- | ---: |
| Main database | 7,288,586,240 bytes |
| WAL sidecar | about 42.7 MB |
| `telemetry_turns` | 6,422,241 rows |
| `telemetry_edits` | 603,105 rows |
| `telemetry_limits` | 267,430 rows |
| free pages | 0 |
| Codex rollout sources | 9,863 files |

The source cursors and `(source_key, event_key)` uniqueness constraints showed
that two mechanisms were involved. First, the same physical Codex rollout was
discovered through multiple projected path aliases, and the path-specific
source key allowed each alias to ingest a second copy. Second, after removing
those aliases, the remaining rows are provider token-delta observations rather
than transcripts. The telemetry design had hourly rollups but no production
retention caller, so both genuine raw facts and alias copies accumulated
indefinitely. Rate-limit observations were also being stored as heartbeat
history even though the UI only reads the newest provider gauge.

## Policy

Wardian retains raw turns, edits, and completed activity intervals for 90 days.
The hourly rollup remains the long-term aggregate representation. The write path
retains only the newest rate-limit gauge per provider. This initial raw window
preserves the detailed dashboard horizon while bounding the largest families in
the database; changing it is a product policy change, not a parser change.

The v4-to-v5 migration canonicalizes physical source paths and collapses facts
with the same `(canonical source, event key)` before normalization. New
discovery and ingest use the same physical identity, so a projected junction
cannot recreate the old multiplication.

The remaining provider callbacks are not a UI data model. Byte-offset providers
(Codex, Claude, and Pi) now persist one source-owned row per five-minute cell,
turn, and model, with additive token fields merged into that row. Edits use the
same grain and retain path/turn/op identity so file counts and line totals stay
exact at the interface's resolution. Timestamp-cursor providers (OpenCode and
the archive fallback) retain event identities because their overlap/rewrite
handling needs them for safe rereads; those sources are much smaller.

Five minutes is the finest Analytics grain, so this preserves the current
matrix cells, distinct turn/file counts, model attribution, and hourly rollup
repair without storing every token callback. The compatibility view still
exposes the existing columns, but its rows are aggregate facts for compacted
providers rather than provider-event transcripts. No provider prompt, response,
patch body, or raw JSON is stored.

## Interface data contract

The Dashboard reads a trailing window when open, on a 15-second backstop and on
telemetry updates. It needs per-agent and per-provider totals plus one sparkline
for the selected measure. Analytics reads on a two-minute backstop and on the
same update event. It needs a bounded rows-by-time matrix for the selected
dimension and measure. Neither surface requests raw provider JSON, prompts,
responses, patch bodies, or individual event records.

The activity view needs interval start/end and method. The provider strip needs
the newest limit gauge. Source cursors and fingerprints are ingestion state,
not interface data, and must remain. This makes the current safe shape:
five-minute source-owned facts for byte-offset providers, event facts only for
timestamp-cursor providers, hourly rollups, activity intervals, latest gauges,
and source cursors. Raw callback rows are no longer required for the main
providers.

Maintenance runs once per day from the desktop application and only after its
managed provider runtimes are quiescent. A due check avoids creating a full
backup when there is nothing to prune. The CLI has no destructive telemetry
maintenance command.

## Recovery and disk behavior

Before deleting any raw fact, the core operation creates and integrity-checks a
full SQLite backup through a temporary sibling path, then atomically promotes
it. It recomputes affected hourly buckets, records a durable prepared cutoff,
deletes in bounded transactions, checkpoints the WAL, and vacuum-compacts the
database. Interrupted deletion resumes at the recorded cutoff and reuses the
newest verified automatic backup rather than creating another full copy for
each retry. Failed temporary outputs are removed. After success, the
application keeps the two newest automatic backups in
`<wardian-home>/backups/telemetry` and rotates only files with its exact backup
prefix.

The first upgraded run may require temporary space for the verified backup and
vacuum. If the backup or maintenance fails, no cleanup is claimed and the
application retries after 15 minutes; the source database is not partially
purged before backup verification.

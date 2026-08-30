# State Database Retention and Compaction Proposal

Status: investigation and proposal only. This document does not delete rows,
checkpoint the live database, or run `VACUUM`.

Issue: [#1069](https://github.com/wardian-app/Wardian/issues/1069)

## Evidence

The measurement used the repository's `rusqlite` dependency through a temporary
read-only Rust probe. It did not run migrations or issue a checkpoint. The
database was live while it was read, so the file size and row counts are a
moving snapshot rather than a quiescent baseline.

During the detailed snapshot:

| Artifact or setting | Observation |
| --- | ---: |
| `<wardian-home>/state.db` | 2,437,533,696 bytes |
| `<wardian-home>/state.db-wal` | 24,262,712 bytes |
| `<wardian-home>/state.db-shm` | 65,536 bytes |
| `journal_mode` | `wal` |
| `wal_autocheckpoint` on the read connection | 1,000 pages |
| `page_size` | 4,096 bytes |
| effective `page_count` | 595,101 |
| `freelist_count` | 0 |
| `auto_vacuum` | 0 (`VACUUM` is needed to reclaim deleted pages) |

A later filesystem read during the same investigation observed the main file at
2,454,401,024 bytes while the WAL remained 24,262,712 bytes. This confirms that
the live writer continued changing the database during measurement. The WAL
size is 5,889 4,120-byte frames plus its 32-byte header. That is an observation,
not proof that every frame is checkpoint-blocked.

`dbstat` was available in the repository-linked SQLite build. The following
family sizes include each table and its indexes; row counts are table rows.

| Table | Rows | Table family bytes |
| --- | ---: | ---: |
| `telemetry_turns` | 2,067,417 | 2,051,325,952 |
| `telemetry_edits` | 226,909 | 299,180,032 |
| `telemetry_limits` | 262,647 | 48,025,600 |
| `events` | 102,855 | 9,797,632 |
| `interactions` | 4,439 | 9,060,352 |
| `telemetry_activity` | 13,271 | 8,212,480 |
| `telemetry_rollup_hourly` | 13,062 | 4,689,920 |
| `telemetry_sources` | 3,303 | 3,211,264 |
| `structured_replies` | 1,107 | 2,224,128 |
| `interaction_delivery_attempts` | 5,295 | 1,654,784 |
| `agents` | 65 | 40,960 |
| `provider_input_state` | 68 | 16,384 |
| `native_deliveries` | 0 | 16,384 |
| `mailbox_messages` | 1 | 12,288 |
| `native_delivery_evidence` | 0 | 12,288 |
| `interaction_events` | 0 | 8,192 |
| `native_session_bindings` | 0 | 8,192 |
| `telemetry_meta` | 1 | 8,192 |

The three largest families account for 98.40% of the measured family bytes.
`telemetry_turns` alone accounts for 84.16%; its family footprint averages
about 992 bytes per row because its table and indexes occupy about 2.05 GB.
The rollup has 13,062 rows versus 2,067,417 raw turns, a 158.3:1 row-count
reduction for the represented additive measures.

The measured timestamp ranges were:

| Table | Earliest | Latest |
| --- | --- | --- |
| `telemetry_turns` | 2026-03-19 | 2026-08-30 |
| `telemetry_edits` | 2026-03-28 | 2026-08-30 |
| `telemetry_activity` | 2026-03-19 | 2026-08-30 |
| `telemetry_limits` | 2026-04-27 | 2026-08-30 |
| `telemetry_rollup_hourly` | 2026-03-19 | 2026-08-30 |
| `events` | 2026-05-04 | 2026-08-30 |

`trajectory_metadata_blob` is not a table in this `state.db`; it is provider-
owned storage and does not explain this file's size.

## Retention path audit

The current implementation does not have an age-retention job:

- `crates/wardian-core/src/db.rs::run_migrations` enables WAL mode but does not
  configure or explicitly invoke a checkpoint. `prune_events` exists, but a
  repository call-site search found no production caller, and it only prunes
  `events` by count.
- `src-tauri/src/state/telemetry_ingest.rs::start_telemetry_ingest` starts an
  immediate background ingest loop and repeats every 60 seconds while an agent
  is live or every 300 seconds while idle. It advances sources and writes raw
  facts; it does not prune by age.
- `crates/wardian-core/src/telemetry/ingest.rs::purge_source_facts` runs only
  when a source is stale because of parser-version or source-fingerprint
  changes. It repairs a source re-ingest and is not retention.
- `crates/wardian-core/src/telemetry/rollup.rs` recomputes hourly rows from the
  raw facts in the same transaction as the ingest cursor. The rollup is derived
  and compact, but it is not a complete replacement for every raw query.

The query contract currently reads additive measures from
`telemetry_rollup_hourly`, but reads raw facts for distinct turns, distinct
files, activity intervals, and rate-limit observations. Deleting raw rows would
therefore change those historical results unless the product explicitly limits
their detailed horizon or adds suitable aggregate structures.

## Proposed policy

This is a proposal for approval, not an instruction to run now.

1. Keep `telemetry_rollup_hourly` for the long term. It is small and preserves
   additive hourly totals, token components, active-time method, and edit line
   totals.
2. Retain raw `telemetry_turns`, `telemetry_edits`, and `telemetry_activity`
   for a documented detail window, with 90 days as the initial candidate. A
   historical view older than that window must either use only the measures the
   rollup can represent or retain the raw rows needed for exact distinct counts
   and timelines.
3. Treat `telemetry_limits` separately. It has no current rollup. Either retain
   its raw observations for the same documented window or add a provider/window
   limit aggregate before pruning it.
4. Keep `telemetry_sources` rows for active or recoverable sources. A source
   cursor must not be discarded if doing so could cause a later rotation or
   parser repair to re-ingest the retained provider history.
5. Do not include `events`, `interactions`, delivery attempts, replies, or
   mailbox data in the first telemetry retention change. They are comparatively
   small and remain evidence for investigations such as #1057.

## Safe maintenance sequence

Any future implementation should be an explicit maintenance operation:

1. Acquire an app-level maintenance lease that prevents agent lifecycle writes,
   telemetry ingest, and other database writers. A normal live session is not a
   safe `VACUUM` window.
2. Perform a dry run reporting the cutoff, candidate row counts, candidate
   bytes, affected rollup buckets, and whether every deleted raw interval has a
   corresponding verified aggregate. Create and verify a backup before the
   first deletion.
3. Delete only approved raw telemetry in bounded batches, preserving source
   cursors and committing each batch. Recompute or verify affected rollups in
   the same transaction boundary as the corresponding batch.
4. Checkpoint the WAL after writers and long-lived readers are quiescent. The
   current source contains no explicit checkpoint path, so a future maintenance
   path must record the checkpoint result and distinguish `busy` from success.
5. Run `VACUUM` only after the app and agents are stopped, the backup is
   verified, and the replacement can be checked before reopening the install.
   With `auto_vacuum=0`, deleting rows and checkpointing alone will leave free
   pages in the main file.

The immediate safe conclusion is that unbounded telemetry fact retention, not
`trajectory_metadata_blob` or control/event history, explains the database
growth. No purge or compaction was performed in this investigation.

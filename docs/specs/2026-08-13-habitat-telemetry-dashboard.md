# Habitat Telemetry: Rollup Store, Dashboard, and Analytics

Filename: `2026-08-13-habitat-telemetry-dashboard.md`

- **Status:** Phases 1 and 2 implemented; Dashboard redesigned around a rows x time matrix; phases 3-4 proposed.
- **Date:** 2026-08-13

## Delivery status

| Area | Outcome |
|---|---|
| Schema | `telemetry/schema.rs`, version 4: seven tables and nine indexes, wired into `db::run_migrations`. Sources are keyed by `(provider, agent session, path)`; facts deduplicate on a non-null `event_key`. A version change rebuilds the tables, which is safe because every row is re-derivable from provider sources. |
| Identity | `telemetry/identity.rs` — FNV-1a record digests and a first-line file fingerprint. Codex keys pair the digest with the record's absolute byte offset, so identity is independent of where a read was cut, of whether the file grew, and of the toolchain that built the binary. |
| Models | `telemetry/models.rs` — facts, rollups, and DTOs. `TokenCounts` is nullable per component and `ActiveTime` has no blended total, so neither "unreported" nor "estimated" can be silently promoted to a measurement. |
| Sources | `TelemetrySource` is "advance this cursor", not "parse this string", because opencode is a live database. `codex.rs` parses `token_count`, `turn_context`, `patch_apply_end`, and `rate_limits`, carrying turn context across deltas; `claude.rs` parses assistant `usage` and file tool calls from Claude Code transcripts; `opencode.rs` reads `message`/`part` read-only. Gemini and antigravity resolve to unsupported. |
| Discovery | `state/telemetry_ingest.rs` — enumerates **every** provider session an agent owns, from the projected habitat unioned with the conversation archive's `provider_session_ids`. Backfill is newest-first under a per-pass byte budget. Replaced live-`resume_session` resolution, which reported each agent's newest conversation as its entire history. |
| Clustering | `activity.rs` — 12 min gap, 40s singleton, carried over from the prior artifact. Anchored on the last stored interval so a session spanning delta boundaries stays one interval, and the anchor is discarded when nothing joins it. |
| Ingest | `ingest.rs` — purge, fact write, cursor advance, and rollup rebuild all commit as one transaction, so a crash can strand neither unwritten facts nor unrebuilt buckets. Replacement (by fingerprint), truncation, and partial trailing lines handled; a parser version bump purges and re-reads. |
| Rollups | `rollup.rs` — dirty-bucket recomputation, idempotent, with intervals clipped to hour boundaries and measured/clustered totals stored apart. |
| Query | `query.rs` — summary, breakdown, series, intervals, and latest-limits, all reading rollups rather than facts. |

Verification: 517 core unit tests plus 20 integration tests, `cargo clippy
--all-targets` clean, `Wardian` and `wardian-cli` still compile. Each regression
test was confirmed to fail against the pre-fix code rather than merely assert the
new code back to itself. The
reconciliation gate passes — ingested opencode per-turn facts equal that
session's own `tokens_*` and `summary_*` columns. Against a **real** codex log,
summed `last_token_usage` deltas exactly reproduce the provider's final
`total_token_usage`: 831,424 input / 730,880 cached / 5,254 output /
2,244 reasoning. That same log confirms the cache trap in production data — 88%
of its input tokens are cache reads. Those figures are also committed as
`tests/fixtures/codex-rollout.jsonl` so the invariant runs in CI, where no real
log exists; the real-log tests remain the only check on format fidelity and
still skip when no log is present.

### Corrections during implementation

Two were caught by the tests written alongside the code:

1. **Clustering fragmented across delta boundaries.** Each ingest cycle sees
   only new bytes, so a continuous work session became one 40s singleton per
   cycle. Clustering is now anchored on the last stored interval.
2. **One cursor across two tables skipped rows.** OpenCode's `message` and
   `part` advance on independent timelines, so parts racing ahead dragged the
   cursor past unread messages. The cursor is now the minimum of the per-table
   maxima, meaning "seen everything up to here, everywhere".

Seventeen more came out of three review rounds, and each is now covered by a
regression test. From the first round:

3. **One cursor served every agent sharing a database.** OpenCode keeps a single
   `opencode.db` per machine. Keyed by path alone, the first agent to ingest
   left its high-water mark as *the* cursor, and the next agent resumed inside
   someone else's history — permanently skipping its own, since the cursor only
   moves forward. Source state is now keyed by provider, agent session, and
   path.
4. **A record split from its `turn_context` deduplicated as two.** Codex states
   turn identity in a separate record, so an incremental read landing between
   the two produced a null `turn_id` where a full re-read produced a real one.
   The old uniqueness key contained that nullable column, and SQL compares
   NULLs as distinct, so a re-read inserted rather than ignored. Facts now carry
   a non-null content-derived `event_key`, and parser context is carried across
   deltas so attribution no longer depends on read timing either.
5. **Replacement was detected only by shrinkage.** A rollout replaced by a file
   the same size or larger resumed at the old byte offset and dropped everything
   before it. A first-line fingerprint now identifies the file; hashing only the
   first line is what keeps ordinary growth from reading as replacement.
6. **The resume anchor inflated the interval before a gap.** The anchor enters
   clustering as a synthetic event; when a long gap meant nothing joined it, it
   came back as a singleton ending 40s later and was written over the stored
   interval. Anchor-only clusters are now discarded.
7. **Estimates and absences were being promoted to measurements.** The rollup
   summed measured and clustered milliseconds into one `active_ms`, directly
   contradicting this spec, and coalesced every token component to zero so an
   unreported metric read as a reported none. `active_ms` is gone and token
   sums are nullable throughout.

From the second round:

8. **A crash could leave rollups permanently behind facts.** Facts and the
    cursor committed in one transaction; the rollup rebuild committed in
    another. A crash in between advanced the cursor past buckets that were never
    recomputed, and since the cursor only moves forward, no later pass would
    revisit them — the surfaces would under-report indefinitely. The purge,
    write, and rebuild now commit as one unit.
9. **A parser fix could not fix anything.** A version bump re-read the source,
    but `INSERT OR IGNORE` collided with the existing rows, so a corrected fact
    could never replace the bad one it was written to repair. A re-read now
    purges that source's facts first.
10. **Clustering still depended on ingest cadence at the gap boundary.** The
    resume anchor used the interval's *credited* end, 40s past the last real
    event, so an event just beyond the 12-minute threshold fell just inside it.
    The same log split into two intervals when read in one pass and merged into
    one when read incrementally. Activity now records `last_event_at` alongside
    `ended_at` and anchors on the former.
11. **Content-only keys merged distinct records and drifted across toolchains.**
    Two byte-identical lines are two events, and `DefaultHasher` is explicitly
    not stable across Rust releases — so a toolchain upgrade would re-key
    everything and a later re-read would duplicate rather than collide. Keys are
    now the record's absolute byte offset plus an FNV-1a digest, pinned in tests
    against published vectors.
12. **Replacement detection had two holes.** A file whose first line was still
    being written yielded no fingerprint, which read as "not stale" and consumed
    the replacement from the middle; and the stored fingerprint was then
    overwritten with nothing, discarding the identity needed to notice later. A
    known identity that becomes unreadable is now stale, and the last known
    fingerprint is retained rather than cleared.
13. **Two code paths disagreed on "billable tokens".** `series` computed it in
    SQL and `TokenCounts::billable_total` in Rust; for a provider reporting only
    cache reads, one said zero and the other said unreported. The SQL now
    mirrors the Rust exactly.

From the third round:

14. **Purging a source left its activity intervals behind.** The justification
    written for this was circular: it argued that re-clustering reproduces the
    same spans, which assumes the re-read yields the same events — false in
    exactly the cases a purge exists for. A parser fix or a replaced log
    produces spans at *different* times, so the old rows never collided, were
    never dirtied, and would have inflated active time permanently. Activity is
    now source-owned and purged with everything else.
15. **A late event could leave two overlapping intervals.** Uniqueness is on
    `started_at`, so a cluster that gained an earlier start did not collide with
    its own previous row; the rollup then clipped both and counted the shared
    minutes twice. Writing a clustered interval now replaces every stored span
    it overlaps, which also subsumes the old widening path — one mechanism
    instead of two.
16. **A source with a first line longer than the fingerprint window could never
    detect replacement.** It fell back to the length check, which only catches a
    file that shrank. The window itself is now the identity in that case, which
    is stable because the files are append-only.
17. **The write helpers did not enforce their own transaction requirement.**
    `write_facts` and `recompute_buckets` were public and took `&Connection`, so
    a future caller could legally advance a cursor and then fail before
    rebuilding rollups — recreating finding 8. Both are now `pub(crate)`.

One further issue was found while reviewing the concurrency question rather than
reported: `ingest_source` used a DEFERRED transaction, which reads before it
writes. Under WAL that upgrade fails with `SQLITE_BUSY_SNAPSHOT` once another
connection has committed — an error waiting cannot resolve. It now begins
IMMEDIATE, so the concurrent scheduler that phase 2 introduces will queue rather
than race. Separately, `wardian-core` sets no `busy_timeout` anywhere; that is
outside this change's scope and is flagged rather than altered.

Phase 2 is implemented. `TelemetryIngestService` (`src-tauri/src/state/telemetry_ingest.rs`)
discovers sources and advances them on its own loop; four commands in
`src-tauri/src/commands/telemetry.rs` read the store; and `DashboardView` is
replaced by a habitat-level telemetry surface.

Two defects surfaced during phase 2 that phase 1's tests could not have caught,
both from the library never having had a caller:

1. `ingest_source` required `&mut Connection`, but the application reaches its
   database through `get_db_conn`, which lends `&Connection` from behind a global
   mutex. The library was uncallable from the app it exists to serve. Every test
   owned its own connection, so nothing noticed. It now takes `&Connection` and
   opens its transaction with `Transaction::new_unchecked`.
2. A synchronous `#[tauri::command]` runs on the **main thread** (the macro's
   default is `ExecutionContext::Blocking`). The read commands take the global
   database mutex, which an ingest pass can hold for as long as its timeout
   allows, so a sync command would freeze the window rather than merely be slow.
   All three reads are `#[tauri::command(async)]` on synchronous bodies, which
   runs them on the runtime's blocking pool.

## Dimensional Model

The rollup's grain is `(hour, agent, provider, model)`. That tuple is the hard
constraint: anything outside it cannot slice a rollup measure.

| Dimension | Grain | Notes |
|---|---|---|
| Time | hour, rolled to local days past 48h | Two roles, and conflating them was the first design's main error: time is both the *window* (a filter) and the *axis* (columns). |
| Agent | `session_id` | The habitat's primary entity. Carries name and class, which the store does not hold — the commands join them from `agents`. |
| Provider | — | Nearly redundant with agent, since an agent has one provider. Earns a pivot only for cross-provider comparison. |
| Model | per turn | Real and interesting. Edits carry no model, so a model view reaches files through the turn. |
| Effort | per turn | Parsed but **not in the rollup grain**. Would require a schema change. |
| Workspace / path | per edit | Asymmetric: can slice edit measures only. Not yet surfaced. |
| Activity method | per interval | Preserved in the store, **deliberately not surfaced** — accurate but not actionable, and it read as clutter. |

Measures fall into three classes, and conflating them caused both of the
correctness defects found when the first Dashboard met real data.

**Additive** — safe to sum across any dimension, and served from the rollup:
`input_tokens` (fresh), `cached_input_tokens`, `cache_write_tokens`,
`output_tokens`, `reasoning_tokens`, `lines_added`, `lines_removed`,
`measured_active_ms`, `clustered_active_ms`, `cost_usd` (opencode only, not
surfaced).

**Distinct counts** — `turns` and `files_touched`. Distinctness does not survive
pre-aggregation: a per-bucket count answers "how many distinct values in this
hour", and no combination of those recovers the global answer. Served from the
fact tables for the window, one indexed query each. A cell may still use a
per-bucket distinct count, because a cell *is* a bucket — which is why a row's
cells do not sum to its total, and why the matrix says so via
`cells_are_not_additive`.

**Gauges** — never summed: `context_window` (max), rate-limit `used_percent`
(latest per account).

A distinct count is non-additive along **both** axes, and this is a property of
the measure rather than a defect. Across buckets: a turn spanning two hours is
one turn but appears in both cells, so `cells_are_not_additive` warns the
surface. Across rows: a file edited by two agents is one file but belongs in
both rows, so a column total exceeds the habitat figure. The matrix therefore
never offers a column total. Both are pinned by tests so a later change to make
them "add up" has to be a deliberate redefinition.

Model attribution for edits deserves a note: edits carry no model, so a model
view reaches it through the edit's turn. That join is taken against **one row
per turn** (`MIN(model)` grouped by `(session_id, turn_id)`), not against the
turn facts directly. Codex writes several token records per turn and they
normally agree on the model, but nothing enforces it; a plain join would emit
one edit under each model and count a single file twice, since
`COUNT(DISTINCT path)` dedupes within a group and not across them.

Ratios must be computed from summed components, never as an average of ratios.

## Two Defects Found Against Real Data

Both were invisible to the phase-1 test suite because the fixtures encoded the
same misunderstanding as the code.

**Tokens overstated 49x.** Codex's `input_tokens` is the whole prompt
*including* the part served from cache — verified on a real log where the first
call reports input 21,804 against cached 11,008. `billable_total = input +
output` therefore counted every cache read twice. On one real habitat that
reported 3.62B tokens where 73.3M were processed. **Opencode is the opposite**:
its `total = input + output + cache.read + cache.write` proves the components
are disjoint. The same column meant two different things depending on who
filled it. `TurnFact::input_tokens` is now a **normalized, cache-exclusive**
quantity, subtracted at ingest by the codex source; `parser_version` 2 forces
every store written by version 1 to be purged and re-read.

The hand-written fixture hid this: it put `input_tokens: 100_544` on a record, a
figure that happens to equal that session's *fresh* total, so the fixture
encoded input as though it were already cache-free and every assertion built on
it agreed with itself. The real-log invariant test passed throughout and was
correct — it checks that summed deltas reproduce the cumulative gauge, which
says nothing about what the components mean.

**Distinct counts overstated.** `turns` and `files_touched` were summed off the
rollup, giving turn-hours and file-hours. A real habitat read 1,813 files where
978 were touched.

## The Discovery Defect: One Conversation Per Agent

Found against the real store after the first Dashboard shipped, and the cause of
every complaint about it — wrong codex figures, a roster that looked like it
contained only codex agents, and opencode agents that appeared dead.

Discovery resolved a source from the agent's **live** `resume_session`, and
`codex_session_file_path` returns exactly one file. But a provider session is not
an agent's history: **codex opens a new rollout file every time an agent
starts**, and opencode opens a new row-set in the shared database. Reading only
the live session reports an agent's newest conversation as the whole of its
past.

Measured on the real habitat:

| | Before | After |
|---|---|---|
| Sources discovered | 34 | **892** |
| Codex rollout files read | 33 of 961 | 860 |
| Claude transcripts read | 0 | 30 |
| Opencode sessions read | 1 of 385 | all, per agent |
| Supported agents with data | ~34 | 42 of 44 |

One agent (`Wardian-Arch`) held 251 rollout files and contributed one. Opencode's
database had been written to the same day while the store's newest opencode turn
was seven weeks old.

**The fix.** Discovery enumerates every session an agent owns, from sources that
are unioned because none is complete alone:

1. **The projected habitat.** Everything under
   `~/.wardian/agents/<uuid>/habitat/<provider-home>/` belongs to that agent by
   construction — no attribution heuristics, and it finds sessions Wardian never
   observed.
2. **The conversation archive.** `provider_session_ids` in each agent's
   `conversations/index.jsonl` already recorded the mapping; it resolves sessions
   written into the *shared* provider home.
3. **The workspace, for opencode.** Opencode has no per-agent home to scan, and
   the archive only knows conversations Wardian captured — so an opencode agent
   with neither resolved to *nothing at all*. It stamps every session with the
   directory it ran in, which attributes the rest the way a projected habitat
   attributes a rollout. Matching is lexical after normalizing separators, case
   and a trailing slash, because opencode writes `D:/Development/x` where Wardian
   holds `D:\Development\x`; comparing raw strings attributes nothing on Windows.
   Verified against the real database: 229 sessions for one workspace, identical
   across all three path spellings.

Codex and claude fan out to one source per file, each with its own byte cursor.
Opencode stays **one** source with one timestamp cursor, carrying the id list
that narrows the shared database to this agent — which is why `SourceContext`
now holds `provider_session_ids` rather than a single id. Its cursor advances to
the **maximum** consumed position across those sessions; taking the minimum would
let one long-finished session pin the cursor in its own past and force every
later pass to re-read the active one from there.

**Backfill is bounded.** The enumerated history is ~9 GB, which cannot be read
before the first paint. Sources are visited newest-first and a pass stops after
`INGEST_BYTES_PER_PASS`, so recent horizons are correct immediately and older
history fills in over following passes. A pass with sources left over reschedules
at a backfill cadence rather than the steady-state one. Budget is charged on
bytes *actually read*, so a source level with its file costs one cursor
comparison and consumes nothing — the steady state still visits everything each
pass.

## Claude Code Source

Seven agents ran on claude and showed nothing, because the provider had no
reader. Its transcripts are the same medium as codex rollouts — append-only
JSONL, one record per line, advanced by byte offset — so the source reuses
`read_delta` and differs only in accounting.

**It reports prompt tokens the opposite way to codex.** Claude follows the
Anthropic API: `input_tokens` already *excludes* everything served from cache,
with `cache_read_input_tokens` and `cache_creation_input_tokens` disjoint beside
it. So this source subtracts nothing, where codex must subtract. On a real
transcript cache reads run **9,617×** fresh input, so getting the direction
wrong is not a rounding error in either direction: summing them would overstate
usage roughly 9,500-fold, and subtracting would clamp nearly every turn to zero.
Both leave every individual figure looking plausible, which is exactly how the
49× codex defect survived. The disjointness is asserted against a real
transcript fixture rather than assumed.

Claude also writes locally generated replies — cancellations, "no response
requested" — as assistant records with all-zero usage under a `<synthetic>`
model. These are not model calls and are excluded, including from the carried
model, so a later record that omits a model does not inherit one that never ran.

## Providers Without a Native Log

Antigravity and gemini publish nothing a parser can read — no token accounting,
no transcript. That was originally recorded as "unsupported", which quietly
became "these agents did nothing". It is the wrong conclusion: **Wardian watched
the work happen.** The conversation archive already stores each turn with its
`turn_key`, `started_at`, `updated_at`, tool calls, and the files it wrote.

`sources/archive.rs` reads `turns.jsonl` from Wardian's own archive and yields
turns, file edits, and clustering timestamps. On this habitat that recovers 784
turns across 31 conversations and 349 distinct written files, for 7 agents that
previously showed empty.

What it deliberately does **not** do:

- **No tokens, no line counts.** Stored as `None`, so they render as unreported.
  A zero would rank antigravity the cheapest provider rather than the unmeasured
  one — the same distinction the nullable `TokenCounts` exists to protect.
- **No measured durations.** A turn is request-to-response and includes however
  long the person took to reply, so its span is not working time. Both endpoints
  feed the clusterer instead, putting active time on the same footing as codex.

Two mechanics are forced by the medium:

1. **Timestamp cursor, not byte offset.** `turns.jsonl` is rewritten atomically
   as turns advance, so a byte offset into it means nothing after the next
   rewrite. Identity comes from the archive's own `turn_key`.
2. **Settle window.** Facts are written `INSERT OR IGNORE`, so the first version
   of a turn is the one that sticks. A turn read mid-flight would be frozen with
   the work it had done so far, and the files it wrote afterwards would never
   appear. Turns quiet for `SETTLE_MS` are ingested; the rest wait.

The fallback is an **allow-list**, never a catch-all, and is consulted only after
the native readers. A provider with its own log is never also read through the
archive, and `mock` is excluded so the test suite's agents stay out of a
habitat's history.

## Context and Problem Statement

Wardian has no historical telemetry. `AgentTelemetry` carries seven
instantaneous fields — `cpu_usage`, `memory_mb`, `uptime_seconds`,
`query_count`, `init_timestamp`, `current_status`, `log_path` — recomputed on
every 5s pass in `manager/telemetry.rs` and then discarded. The SQLite schema in
`wardian-core/src/db.rs` has eight tables and not one of them stores a measure
over time.

Two consequences follow.

First, the Dashboard surface has nothing aggregate to show, so it shows a
vertical list of agent cards instead. That list duplicates the Agents overview
grid and the right-hand Roster. Three surfaces render the same per-agent status
in three shapes, and none of them answers "how much work is my habitat actually
doing".

Second, any attempt at real analysis has to go around the app. The prior
activity artifact reconstructed seven days of per-agent active time by scraping
provider session logs directly, clustering per-event timestamps, and decoding
Antigravity's protobuf session database. It produced the right numbers from the
right source. As an execution model it does not survive contact with a live
surface: it re-reads gigabytes on every run, and `manager/telemetry.rs` already
tail-bounds log parsing to 4 MiB (`LOG_PARSE_TAIL_BYTES`) precisely because
codex logs reach hundreds of megabytes.

The data the user asked for — active time, token usage, model usage, files and
lines edited, across multiple time horizons — already exists on disk. What is
missing is a store, an ingest path, and two surfaces with distinct jobs.

## Evidence: what provider sources actually contain

Verified directly against live provider state on this machine on 2026-08-13.
Every claim below was checked against real data; the two gaps are labelled as
gaps rather than guessed at.

The first finding is structural: **provider sources are not all append-only log
files.** Two are SQLite databases. That invalidates a byte-offset-only ingest
model and is reflected in the schema below.

| Provider | Source kind | Location |
|---|---|---|
| codex | append-only JSONL | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| claude | append-only JSONL | `~/.claude/projects/<slug>/<uuid>.jsonl` |
| antigravity | JSONL + SQLite | `…/brain/<id>/.system_generated/logs/transcript.jsonl`, `…/conversations/<id>.db` |
| opencode | **SQLite** | `~/.local/share/opencode/opencode.db` (66 MB, WAL) |
| gemini | — | out of scope; no longer actively supported |

**Codex** (JSONL, one object per line, each with a top-level `timestamp`):

| Record | Supplies |
|---|---|
| `event_msg` / `token_count` | `total_token_usage` and `last_token_usage`, each with `input_tokens`, `cached_input_tokens`, `cache_write_input_tokens`, `output_tokens`, `reasoning_output_tokens`, `total_tokens`; plus `model_context_window` |
| `token_count` → `rate_limits` | `primary.used_percent`, `primary.window_minutes`, `resets_at`, `plan_type`, credit balance |
| `turn_context` | `turn_id`, `model`, `effort`, `cwd`, `workspace_roots`, `collaboration_mode.settings.model` |
| `event_msg` / `patch_apply_end` | `turn_id`, `success`, and a `changes` map keyed by absolute path. `{"type":"add","content":…}` gives full text; `{"type":"update","unified_diff":…,"move_path":…}` gives a hunk. Line counts are computable for both. |
| `session_meta` | session start timestamp |

Codex edit coverage was measured rather than assumed. Across 57 August session
logs: 874 `exec` tool calls, of which 59 invoked `apply_patch` — and exactly 59
change entries appear in `patch_apply_end` (57 `add`, 2 `update`). Capture of
provider-native patching is therefore **1:1**. A further 17 `exec` calls used
shell write primitives (`> ` ×9, `tee ` ×6, `node -e`, `python -c`) that can
write files without producing a `patch_apply_end` record at all. Note the tool
payload field is `input` (a JS snippet calling `tools.shell_command`), not
`arguments`.

**Claude** (JSONL): per-message `model` and `usage` with `input_tokens`,
`cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`,
`output_tokens_details.thinking_tokens`. Edits appear as `tool_use` blocks —
`Edit` with `{file_path, old_string, new_string, replace_all}` and `Write` with
`{file_path, content}`. No rate-limit data of any kind. Roughly a third of
records (metadata kinds such as `custom-title`, `mode`, `agent-name`) carry no
timestamp and must be filtered out before activity clustering.

**OpenCode** (SQLite) is by a wide margin the richest source, and needs almost
no parsing. Its `session` table stores per session: `model` (JSON with `id`,
`providerID`, `variant`), `agent`, `directory`, `cost`, `tokens_input`,
`tokens_output`, `tokens_reasoning`, `tokens_cache_read`, `tokens_cache_write`,
and — already aggregated — `summary_additions`, `summary_deletions`,
`summary_files`. Its `message` table carries per-turn `tokens` (with a nested
`cache.{read,write}`), `cost`, `modelID`, `providerID`, `path.cwd`, `finish`,
and **`time.created` plus `time.completed`**, giving exact turn durations rather
than inferred ones. The `part` table holds tool calls (`edit` ×245,
`apply_patch` ×163, `write` ×35) and `patch` parts carrying a git hash with a
touched-file list. Timestamps are epoch milliseconds. An event-sourced `event`
table with a monotonic per-aggregate `seq` exists and offers a natural ingest
cursor. Current store: 381 sessions, 4,074 messages, 15,033 parts.

**Antigravity**: the transcript is fully timestamped — 1,478 of 1,478 records
carry `created_at`, at one-second granularity — with `source`, `type`, `status`,
`tool_calls`. `CODE_ACTION` records (91 in the sampled conversation) identify
edited files as `file://` URIs inside prose (`"Created file file:///D:/… with
requested content"`), with no diff and no line counts. Model is recoverable only
by extracting strings from the protobuf blobs in the conversation database's
`gen_metadata` table (`gemini-3-flash-a`, `Gemini 3.5 Flash (High)`,
`gemini-pro-default`, plus `used_claude` / `used_non_gemini_model` flags).
**There is no token accounting anywhere in antigravity.** An exhaustive scan of
every table for `token|usage|input_tok|output_tok` returned only incidental
prose from model reasoning text and Google API pagination tokens.

This is a known property of the tool, not a gap in this investigation.
[ccusage's source-support Q&A][ccusage-qa] reached the same conclusion
independently: Antigravity's `.pb` files "are opaque binary payloads and do not
expose readable token usage, model usage, or per-turn accounting without
Antigravity's private schema and storage semantics", and "do not include input,
output, cache, or reasoning token counts". ccusage lists Antigravity as
unsupported for exactly this reason. Our finding that model *names* are
extractable as loose strings while structured accounting is not is consistent
with that description. Treat antigravity token support as closed unless
Antigravity publishes a schema.

[ccusage-qa]: https://github.com/ccusage/ccusage/blob/0529319fcbea4e30e63a395daa9a14ae4917df51/docs/guide/source-support-qa.md

**Gemini is out of scope.** It is no longer actively supported in Wardian, and
there is no data to build against regardless: all gemini chat data on this
machine totals 1,467 bytes across four files, every one a synthetic fixture left
by Wardian's own provider probes. It gets no telemetry source, and the registry
reports it as unsupported.

Resulting capability matrix. "Exact" and "inferred" distinguish measured
durations from gap-clustered ones:

| Provider | Timestamps | Active time | Tokens | Model | Edits | Lines | Limits |
|---|---|---|---|---|---|---|---|
| opencode | yes (epoch ms) | **measured** | yes, 5-way | yes, per turn | yes, pre-aggregated | yes, pre-aggregated | none |
| codex | yes | clustered | yes, 5-way | yes, per turn | yes, 1:1 for `apply_patch` | yes | rate-limit % + plan |
| claude | yes¹ | clustered | yes, 5-way | yes, per message | yes, tool inputs | computed² | none |
| antigravity | yes | clustered | **none**³ | protobuf strings⁴ | file paths only | **none** | none |

¹ After filtering untimestamped metadata records.
² From `old_string`/`new_string`; `replace_all: true` applies one edit N times,
so counts are approximate for those.
³ Corroborated by ccusage, which lists Antigravity as unsupported for this
reason. Closed unless the format is documented.
⁴ Fragile string extraction from an undocumented binary format.

Gemini is omitted rather than listed as unknown: it is not actively supported
and has no data to characterise.

## Surface boundaries

Three surfaces, three jobs. The boundary that matters most is that **Dashboard
does not triage**.

| Surface | Tense | Job | Unit |
|---|---|---|---|
| **Inbox** | imperative | "This agent is blocked on you." Route attention to individual items. | one queue item |
| **Dashboard** | present + recent | "What is my habitat doing, and how much of it." Aggregate behavior, fixed opinionated layout. | the habitat |
| **Analytics** | past | "Why, broken down how I choose." Free-form pivot across dimension × metric × horizon. | any grouping |

Dashboard carries no "needs you" list and no action-required counters. A count
of blocked agents that links into Inbox is Inbox's job wearing a different hat,
and the two would drift. Dashboard reports work performed; Inbox reports work
requested.

### Dashboard layout

> **Superseded.** The Dashboard is now specified as a live fleet monitor in
> [2026-08-14-dashboard-fleet-monitor.md](2026-08-14-dashboard-fleet-monitor.md).
> The per-agent table below was the third rejected attempt: the *unit* was right,
> but every figure was a cumulative total over a horizon, which is a historical
> ranking rather than a monitor. The store, ingest and Analytics sections of this
> spec still stand. The rest of this section is kept as the record of what was
> tried and why it failed.

**Superseded twice before landing.** Both rejected designs are recorded here
because the reasons they failed are the constraints the built one satisfies.

- *Attempt 1 — panel stack.* A habitat-wide aggregate strip over per-measure
  panels. Rejected: the aggregate strip duplicates Inbox's job, per-agent figures
  were only available for active time, and rows rendered session UUIDs.
- *Attempt 2 — the matrix.* One rows × time heatmap for a single chosen measure.
  Rejected harder: it answered one measure at a time where the question is
  multi-measure, and its row limit showed 4 agents out of 54.

What both got wrong is that a dashboard row has to carry **several measures at
once and a sense of time**, and that the roster is the answer rather than a
top-N of it.

The built layout, one row per agent:

| Element | Job |
|---|---|
| **Agent** | Name and class. Never the session UUID — every row goes through `label_for`. |
| **Trend** | The sorted measure bucketed across the window, as a sparkline. |
| **Active / Turns / Tokens / Files / Lines** | Five measures side by side, each sortable. |
| **Drill-through** | Opens Analytics pre-scoped to that agent. |

Three rules hold it together:

1. **The sparkline follows the sort.** Choosing a column switches the trend to
   that measure, so the shape beside a row always belongs to the number being
   read. A sparkline of an unrelated measure is decoration.
2. **One scale across the table.** Sparklines normalize against the busiest
   bucket in the *table*, not the row. Per-row normalization would draw an agent
   that worked ten minutes and one that worked all week as the same shape.
3. **Every agent is listed.** Quiet agents are grouped under "Nothing recorded in
   this window", not dropped. A roster that hides its quiet members makes the
   habitat look smaller than it is.

Bucket width is chosen by **count, not fixed width** — the finest grain fitting
`SPARK_BUCKETS` columns. A fixed hourly floor would draw a four-hour horizon as
four columns, which is a bar chart with no shape.

**Providers** is a structural element above the table: one row per provider used
in the window, with its agent count, active time, tokens, and account capacity.

Capacity was first built as a component of its own, rendered only when a limit
existed. That was wrong in a way worth recording: **only codex publishes a
limit**, so the Dashboard grew and lost an entire block depending on which
provider a habitat happened to run. The layout was contingent on one vendor,
which is what made it read as arbitrary — a better label would not have fixed it.

As a field on a provider row it is simply absent for providers that do not report
one, exactly as any other unreported measure is absent. It stays out of the agent
table because it is not an agent measure: two agents on one account observe the
same figure, so it is never summed and never shown per row.

### Analytics layout

Its own surface (`Mod+Alt+A`, command palette, Home), holding the axis you read
values off: a rows × time matrix over a chosen dimension, measure and horizon,
at the full adaptive grain. The Dashboard owns the *shape you notice*; Analytics
owns the *values you look up*. Drill-through from a Dashboard row carries
`focus_key`, so arriving from an agent highlights that row rather than dropping
the reader on a default view.

## Architecture

### Rejected alternative: query the logs on demand

Keeping the artifact's model — parse provider logs when a surface renders —
fails on cost. One agent produced 38,931 timestamped events in seven days in the
prior artifact's own data, across 52 agents, against files that reach hundreds
of megabytes. The existing 4 MiB tail bound exists to avoid exactly this and
would have to be abandoned to get historical coverage. Parse once, store
normalized, query the store.

### Placement

Pure logic goes in `wardian-core` so the CLI inherits it; scheduling and IPC
stay in `src-tauri`. This follows the existing split and the planned CLI work
over the core library.

```
crates/wardian-core/src/telemetry/
  mod.rs          schema bootstrap, migrations
  models.rs       DTOs (snake_case serde)
  sources/        one module per provider, behind a shared trait
    mod.rs        TelemetrySource trait + registry (gemini: unsupported)
    codex.rs      claude.rs  antigravity.rs   JSONL, pure parse_delta
    opencode.rs                               SQLite, cursor over time_updated
  activity.rs     interval clustering (clustered method only)
  rollup.rs       dirty-bucket recomputation
  query.rs        summary / series / breakdown reads

src-tauri/src/state/telemetry_ingest.rs   background scheduler
src-tauri/src/commands/telemetry.rs        Tauri commands
```

### Schema

Fact tables are append-only. Rollups are derived and recomputable from facts, so
a rollup bug is fixed by recomputation rather than migration. The whole store is
derived from provider sources that remain on disk, which is why a schema version
change rebuilds rather than migrates: the cost is one re-ingest.

```sql
-- Ingest watermarks, one row per source *per agent*.
--
-- The key is deliberately not the path. OpenCode keeps a single database for
-- every agent on the machine, so a path-keyed cursor would be shared between
-- agents whose histories are unrelated, and the second one to ingest would
-- resume inside the first one's timeline.
--
-- `cursor_kind` selects how `cursor_value` is interpreted, because a byte
-- offset is meaningless for a database source. `fingerprint` identifies the
-- bytes a byte offset refers to; `carry_*` is parser state that outlives a
-- delta, so a record's attribution does not depend on when ingest ran.
CREATE TABLE IF NOT EXISTS telemetry_sources (
    source_key          TEXT PRIMARY KEY,   -- provider | session_id | path
    source_path         TEXT NOT NULL,
    session_id          TEXT NOT NULL,
    provider_session_id TEXT,
    provider            TEXT NOT NULL,
    source_kind         TEXT NOT NULL,      -- jsonl | sqlite
    cursor_kind         TEXT NOT NULL,      -- byte_offset | epoch_ms | event_seq
    cursor_value        INTEGER NOT NULL DEFAULT 0,
    last_size           INTEGER NOT NULL DEFAULT 0,
    last_modified       TEXT,
    last_ingested_at    TEXT,
    parser_version      INTEGER NOT NULL DEFAULT 1,
    fingerprint         TEXT,               -- hash of the source's first line
    carry_turn_id       TEXT,
    carry_model         TEXT,
    carry_effort        TEXT,
    carry_cwd           TEXT
);

-- One row per completed provider turn.
--
-- `event_key` is derived from the record's own content and is NOT NULL, which
-- is what makes re-ingest idempotent. A key containing the nullable `turn_id`
-- would not: codex states turn identity in a separate record, so the same event
-- can parse with or without it depending on where a delta was cut, and SQL
-- compares NULLs as distinct — so the constraint would admit the duplicate
-- rather than reject it.
CREATE TABLE IF NOT EXISTS telemetry_turns (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    event_key           TEXT NOT NULL,
    session_id          TEXT NOT NULL,
    provider            TEXT NOT NULL,
    turn_id             TEXT,
    model               TEXT,
    effort              TEXT,
    started_at          TEXT,
    ended_at            TEXT NOT NULL,
    input_tokens        INTEGER,            -- NULL when unreported, never 0
    cached_input_tokens INTEGER,
    cache_write_tokens  INTEGER,
    output_tokens       INTEGER,
    reasoning_tokens    INTEGER,
    context_window      INTEGER,
    cost_usd            REAL,               -- opencode only; NULL elsewhere
    source_key          TEXT NOT NULL,
    source_path         TEXT NOT NULL,
    UNIQUE(source_key, event_key)
);

-- One row per file changed by a provider-native patch application.
CREATE TABLE IF NOT EXISTS telemetry_edits (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    event_key     TEXT NOT NULL,
    session_id    TEXT NOT NULL,
    provider      TEXT NOT NULL,
    turn_id       TEXT,
    occurred_at   TEXT NOT NULL,
    workspace     TEXT,
    path          TEXT NOT NULL,
    op            TEXT NOT NULL,          -- add | update | delete
    lines_added   INTEGER,
    lines_removed INTEGER,
    source_key    TEXT NOT NULL,
    source_path   TEXT NOT NULL,
    UNIQUE(source_key, event_key)
);

-- Clustered active intervals.
CREATE TABLE IF NOT EXISTS telemetry_activity (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL,
    provider    TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    ended_at    TEXT NOT NULL,
    event_count INTEGER NOT NULL,
    -- measured: real start/end from the provider (opencode turn times).
    -- clustered: inferred by gap-clustering event timestamps.
    -- decoded: recovered from a binary session store.
    method      TEXT NOT NULL,            -- measured | clustered | decoded
    UNIQUE(session_id, started_at)
);

-- Account-level rate limit gauge observations.
CREATE TABLE IF NOT EXISTS telemetry_limits (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    provider       TEXT NOT NULL,
    limit_id       TEXT,
    observed_at    TEXT NOT NULL,
    used_percent   REAL,
    window_minutes INTEGER,
    resets_at      TEXT,
    plan_type      TEXT
);

-- Derived hourly rollup. Every surface query reads here, never the facts.
--
-- Two deliberate absences. There is no blended `active_ms`, because a column
-- holding measured + clustered would be read as authoritative by everything
-- downstream while containing an estimate wherever a provider could not report
-- real durations. And token columns are nullable rather than
-- NOT NULL DEFAULT 0, so a component nothing reported stays absent; `SUM` over
-- all-NULL input already yields NULL, so this needs no special handling, only
-- the absence of a COALESCE.
CREATE TABLE IF NOT EXISTS telemetry_rollup_hourly (
    bucket_start        TEXT NOT NULL,
    session_id          TEXT NOT NULL,
    provider            TEXT NOT NULL,
    model               TEXT NOT NULL DEFAULT '',
    measured_active_ms  INTEGER NOT NULL DEFAULT 0,
    clustered_active_ms INTEGER NOT NULL DEFAULT 0,
    turns               INTEGER NOT NULL DEFAULT 0,
    input_tokens        INTEGER,
    cached_input_tokens INTEGER,
    cache_write_tokens  INTEGER,
    output_tokens       INTEGER,
    reasoning_tokens    INTEGER,
    tokens_reported     INTEGER NOT NULL DEFAULT 0,
    files_touched       INTEGER NOT NULL DEFAULT 0,
    lines_added         INTEGER NOT NULL DEFAULT 0,
    lines_removed       INTEGER NOT NULL DEFAULT 0,
    cost_usd            REAL,
    PRIMARY KEY (bucket_start, session_id, provider, model)
);

CREATE INDEX IF NOT EXISTS idx_rollup_bucket ON telemetry_rollup_hourly(bucket_start);
CREATE INDEX IF NOT EXISTS idx_turns_session_end ON telemetry_turns(session_id, ended_at);
CREATE INDEX IF NOT EXISTS idx_edits_session_time ON telemetry_edits(session_id, occurred_at);
CREATE INDEX IF NOT EXISTS idx_activity_session ON telemetry_activity(session_id, started_at);
```

Hourly buckets keep 7d at 168 rows per agent per provider per model. The prior
artifact's 38,931 raw events for a single agent-week collapse into at most 168.

### Ingest pipeline

> **Implemented:** the per-source advance below (`ingest_source`), including
> replacement detection, delta reading, parsing, fact writing, and dirty-bucket
> recomputation.
>
> **Phase 2, implemented:** `TelemetryIngestService`, source discovery, and
> cadence. **Still not built:** backfill and retention, which are deferred
> because neither is needed to make the store correct — see the notes below.

`TelemetryIngestService` owns a `tokio` task on its own cadence, deliberately
separate from the 5s telemetry pass. The 5s pass drives live status and is
latency-critical; the recent `perf(workbench): optimize full-surface agent
scale` work exists to protect it. Ingest must never share its thread or its
lock.

Each cycle will **discover** sources by reusing the path resolution already in
`manager/telemetry.rs` (`codex_session_file_path`, the Claude project-dir
convention, `AntigravityProvider::conversation_log_path`,
`opencode_database_path`), then advance each source according to its kind.

**JSONL sources** (codex, claude, antigravity transcript):

1. **Detect replacement.** Compare the source's first-line fingerprint against
   the stored one; a mismatch, or a known fingerprint that has become
   unreadable, resets the cursor to 0. A length check alone is not enough — it
   catches a file that shrank and misses one replaced by something the same size
   or larger, which would then be consumed from the middle. Hashing only the
   first line is what keeps ordinary appends from reading as replacement.
2. **Read the delta only** — `seek(cursor_value)` to EOF. Discard a trailing
   partial line and leave the cursor at the last complete newline, so a
   half-written record is re-read next cycle rather than parsed as garbage.

**SQLite sources** (opencode, antigravity conversation database): open
read-only, and select rows newer than the cursor — for opencode,
`WHERE time_updated > ?` against `message` and `part`. Read-only access must not
disturb the provider's own WAL: open with `SQLITE_OPEN_READ_ONLY`, matching the
existing `opencode_last_assistant_text_from_db`, and never open with
`immutable=1`, which would silently skip WAL contents and under-report recent
activity. A busy database is skipped and retried next cycle rather than waited
on.

Both kinds then converge:

3. **Parse** the delta through the provider's `TelemetrySource` implementation.
4. **Purge, write, and recompute in one transaction** — on a re-read, drop the
   facts this source previously produced; write the new facts and the cursor;
   rebuild every dirty bucket. All of it commits together.

   Both halves matter. The purge is what makes a parser fix a *fix*:
   `INSERT OR IGNORE` cannot correct a row that already exists, so without it the
   re-read collides with exactly the bad facts it was meant to replace. And the
   single commit is what keeps the cursor honest: committing facts before the
   rollup rebuild would let a crash in between strand the cursor ahead of buckets
   that were never recomputed, and since the cursor never moves back, no later
   pass would notice.

**Cadence — implemented.** 60s while any agent is non-idle, 300s otherwise, with
an immediate first pass at startup rather than one interval of blank Dashboard.
A pass is abandoned after 120s so a wedged source cannot hold the loop. The
`telemetry_refresh` command gives a surface an on-demand pass.

Discovery deliberately does **not** filter on `is_off`. An agent's log holds work
done while Wardian was closed, so skipping off agents would make recorded history
depend on whether the app happened to be running — which is the exact property
this store exists to stop being true. `is_off` informs cadence only.

**Backfill — deferred, and probably unnecessary.** The design assumed a bounded
walk-back governed by `WARDIAN_TELEMETRY_BACKFILL_DAYS`. In practice a source's
cursor starts at zero, so the first pass over any source already reads it whole:
backfill is what ingest does by default, not a separate mode. What the setting
would actually buy is a way to *limit* that first read, which is a performance
guard rather than a feature, and no observed log is large enough to need one yet.
Revisit if a first pass is measured to stall.

**Retention — deferred.** Facts older than 180 days were to be pruned. Nothing
prunes yet, and nothing needs to: the store is days old. This is tracked rather
than silently dropped, because the fact tables do grow without bound and the
decision only gets harder once there is data worth losing.

### Source contract

Two source kinds means the contract cannot be "parse this string". It is
"advance this cursor and hand back facts".

```rust
pub trait TelemetrySource: Send + Sync {
    fn provider(&self) -> &'static str;
    fn parser_version(&self) -> u32;
    fn cursor_kind(&self) -> CursorKind;

    /// Read everything after `cursor` and return facts plus the new cursor.
    /// Implementations never write to the telemetry store.
    fn read_since(&self, ctx: &SourceContext, cursor: Cursor)
        -> Result<(ParsedFacts, Cursor), SourceError>;
}

pub struct ParsedFacts {
    pub turns: Vec<TurnFact>,
    pub edits: Vec<EditFact>,
    /// Measured intervals where the provider reports real start/end times
    /// (opencode). Emitted directly, bypassing clustering.
    pub intervals: Vec<IntervalFact>,
    /// Bare event timestamps, fed to the clusterer for providers that
    /// cannot report durations.
    pub event_times: Vec<DateTime<Utc>>,
    pub limits: Vec<LimitObservation>,
}
```

JSONL implementations delegate to a pure `parse_delta(&str) -> ParsedFacts`,
keeping the format logic table-testable against captured fixtures with no
filesystem and no database. That separation matters because this is five
implementations against five independently evolving external formats, three of
which are undocumented.

A source that reports both `intervals` and `event_times` is a bug; the
clusterer's input is exactly the providers that cannot do better.

## Metric definitions

Precision here is the difference between a dashboard and a decorative one.

**Active time** is computed by one of two methods, and they are not equivalent.

*Measured* — opencode reports `time.created` and `time.completed` per message,
so its active time is the summed real duration of its turns. No inference.

*Clustered* — codex, claude, and antigravity report event timestamps but no
durations. Consecutive events within `ACTIVE_GAP_THRESHOLD` (12 minutes, carried
over from the prior artifact) form one interval; a single-event interval
contributes `ACTIVE_SINGLETON_MS` (40s, matching the artifact's convention).
This is neither wall-clock uptime nor CPU time; it is "the provider was emitting
events", and it systematically overestimates relative to the measured method
because it counts think-time gaps under the threshold as active.

Because the two methods differ in kind, a habitat total that sums them is
comparing unlike quantities. Every interval carries its `method`, clustered
intervals stay visually distinct in the UI as the prior artifact did with its
dashed treatment, and any cross-provider active-time comparison labels the
mixture rather than presenting a single authoritative figure. Antigravity's
one-second timestamp granularity is a further floor on its precision.

**Tokens.** Summed from per-turn `last_token_usage`, never from
`total_token_usage` — the latter is a session-cumulative gauge and summing it
across turns multiplies the true figure. Invariant worth asserting in tests: for
a fully ingested codex session, the final `total_token_usage.total_tokens`
equals the sum of ingested per-turn totals.

`cached_input_tokens` is reported as its own series and never folded into
`input_tokens`. Two independent measurements confirm the scale of the error this
avoids: in the sampled codex turn, 730,880 of 831,424 input tokens were cache
reads; across opencode's full 381-session store, 245.6M cache-read tokens
against 24.9M input tokens — a factor of ten. Folding them together would make
every intensity or cost reading meaningless.

Antigravity contributes **no** token data. Its rows are `NULL`, never `0`, and
the UI distinguishes "not reported" from "zero". A provider that cannot report
tokens must not appear as the cheapest one.

**Model usage** is attributed per turn, never per session. The verified
`turn_context` carries both a top-level `model` and a
`collaboration_mode.settings.model`, and either can change mid-session.

**Files and lines** are a **lower bound**, and the UI must label them as such.
The bound is now quantified rather than asserted: codex captures its
provider-native patching 1:1 (59 of 59), but 17 further `exec` calls in the same
sample used shell redirection or interpreter one-liners that write files without
any patch record. Antigravity yields file paths with no line counts at all, so
its line figures are `NULL`. Where a workspace is a git repository, the existing
numstat parsing in `commands/git.rs` offers an independent cross-check, deferred
to phase 4.

**Cost is captured but not surfaced.** Only opencode reports a real USD `cost`;
codex, claude, and antigravity report none. Any figure Wardian displayed would
be one provider's spend wearing four providers' clothes, so no cost appears in
Dashboard or Analytics and there is no habitat-level currency total. The
`cost_usd` column is still populated at ingest, because discarding a field the
provider hands over for free is irreversible and re-ingesting to recover it is
not. Surfacing it stays available if provider coverage ever improves.

**Rate limits** are what Dashboard shows instead, and only where reported —
codex supplies `rate_limits.primary.used_percent` with `resets_at` and
`plan_type`; no other provider supplies anything. On subscription plans this is
the more actionable signal anyway.

Limits are an **account-level gauge**, not an agent measure,
and must never be summed or averaged across agents. Reads take the most recent
observation per `(provider, limit_id)`. Two agents on one codex account
observing 43% are observing the same 43%.

### Commands and DTOs

Tauri commands in `commands/telemetry.rs`, `snake_case` properties per the DTO
convention:

| Command | Returns |
|---|---|
| `get_telemetry_summary(horizon, scope)` | aggregate measures for the horizon |
| `get_telemetry_series(horizon, bucket, group_by)` | time series for timeline and stacked panels |
| `get_telemetry_breakdown(horizon, dimension)` | ranked rows for provider and top-mover panels |
| `get_telemetry_activity_intervals(horizon, session_ids)` | Gantt intervals with `method` |
| `get_provider_limits()` | latest observation per provider |
| `run_telemetry_ingest()` | forces a pass; returns sources scanned and bytes read |

### Frontend

Two surfaces registered in `CORE_SURFACE_CONTRIBUTIONS` under `Core views`.
`dashboard` keeps its `surface_type` so persisted workbench layouts survive;
its view is replaced. `analytics` is new, with `requires_resource: false` and an
optional persisted scope in surface state so a provider drill-through restores.

```
src/features/analytics/
  useTelemetryStore.ts        Zustand: horizon, scope, cached responses
  telemetryFormat.ts          duration, token, and delta formatting
  panels/                     ActivityTimeline, ProviderBreakdown, TokenComposition,
                              HorizonAggregates, TopMovers, LimitHeadroom
src/views/DashboardView.tsx   replaced
src/views/AnalyticsView.tsx   new
```

Charts follow the `dataviz` skill and semantic theme variables per the UI
standards — no hardcoded Tailwind colors. Provider series colors carry over from
the prior artifact.

The existing `DashboardView.test.tsx` asserts the card list and will be replaced
alongside the view.

## Phasing

Sequencing follows the evidence: opencode moves into phase 1 because it is the
cheapest source to ingest and the only one with measured durations and real
cost, which makes it the reference implementation the inferring providers are
validated against.

| Phase | Scope | Exit condition |
|---|---|---|
| 1 | Core **library**: schema, `ingest_source`, dual-kind cursors, **opencode + codex** sources, clustering, rollups, query layer | Codex token invariant holds against a real log and a committed fixture; opencode rollups reconcile against its own `session` aggregates; incremental and single-pass ingest agree. **Met.** |
| 2 | `TelemetryIngestService`, source discovery, Tauri commands, CLI summary, then Dashboard replacement on phase 1 data | `wardian telemetry summary` returns correct figures; Dashboard renders a rows x time matrix over any measure; old card list removed. **Met.** |
| 3 | Claude source, then Analytics surface with pivot and drill-through | Provider row in Dashboard opens scoped Analytics |
| 4 | Antigravity source (timestamps and edits; tokens permanently out of scope) and git numstat cross-check for lines | Antigravity active time renders as clustered, tokens render as "not reported" |

Phases 1 and 2 are independently shippable. Opencode's pre-aggregated
`summary_additions` / `summary_deletions` / `summary_files` give phase 1 a
self-check no other provider offers: ingested per-turn facts must reconcile
against the provider's own session totals.

Gemini gets no phase. It is not actively supported, and the only data available
to build against is four synthetic probe fixtures — a parser written against
those would pass its own tests and break on contact with real sessions. The
registry reports it unsupported.

## Risks

| Risk | Mitigation |
|---|---|
| Provider formats change without notice | Sources are versioned and the JSONL parsers are pure; a format break degrades one provider's metrics rather than failing ingest. Fixtures captured from real logs make breakage visible in CI. Codex has already moved shell invocation from `arguments` to an `input` field holding a JS snippet, so this is observed behaviour, not a hypothetical. |
| Reading opencode's live database corrupts or blocks it | Read-only handles only, never `immutable=1` (which would skip WAL and under-report), no writes, and a busy database is skipped rather than waited on. The existing `opencode_last_assistant_text_from_db` already reads this database safely in production. |
| Antigravity model extraction breaks | It scrapes strings from an undocumented protobuf blob and is the most fragile path in the design. It degrades to "model unknown" and never fails ingest. Antigravity active time does not depend on it. |
| Mixed active-time methods read as comparable | Method is stored per interval, rendered distinctly, and labelled wherever providers are compared. |
| Backfill stalls first launch after upgrade | Not built, and on reflection largely already handled: a source's cursor starts at zero, so the first pass reads it whole by default. Passes run off the telemetry thread with a 120s abandon. Revisit if a first pass is measured to stall. |
| `turns` over-counts across bucket boundaries | Known and structural. The rollup counts distinct `turn_id` *per hourly bucket and per model*, so a turn crossing an hour boundary is counted in both, and one that switches model mid-turn is counted on both rows. A global `COUNT(DISTINCT ...)` is not recoverable from pre-aggregated counts at all — the only exactly-additive alternative is counting model calls, which answers a different question. Error is bounded by boundaries crossed, so it is small for turns measured in minutes. Pinned by `a_turn_crossing_an_hour_boundary_is_counted_in_both_buckets` so a future change to the definition has to be a decision rather than drift. |
| Ingest contends with the 5s telemetry pass | Separate task and cadence. Ingest holds the SQLite write lock only for short per-source transactions. |
| A read command freezes the window | A synchronous `#[tauri::command]` runs on the **main thread**, and every read here takes the global database mutex an ingest pass can hold. All three reads are `#[tauri::command(async)]` over synchronous bodies, which runs them on the runtime's blocking pool. |
| Store growth on a 52-agent habitat | Hourly rollups keep reads cheap. Fact retention is designed but **not implemented**, so the fact tables currently grow without bound; they are prunable whenever it is built, because rollups are self-sufficient. |
| Edit counts read as authoritative | Labelled as a lower bound in the UI, with the mechanism stated in the panel's help text. |
| Double counting on re-ingest | `UNIQUE(source_key, event_key)` on turns and edits, and `UNIQUE(session_id, started_at)` on activity, make re-ingest idempotent. `event_key` is NOT NULL and derived from the record's own position and content, never from a nullable column the parser might or might not have populated — SQL compares NULLs as distinct, so such a key would admit duplicates rather than reject them. |
| A parser fix cannot repair what it already got wrong | A version bump purges that source's facts inside the same transaction as the re-read, because `INSERT OR IGNORE` would otherwise collide with the very rows being corrected. |

## Testing

- **Core unit** — parser table tests against captured fixtures per provider;
  clustering tests covering gap boundaries, singletons, and out-of-order
  timestamps; rollup recomputation idempotence; the token-sum invariant;
  line counting from codex `unified_diff` and `content`, and from claude
  `old_string`/`new_string` including the `replace_all` case.
- **Reconciliation** — ingested opencode per-turn facts must sum to that
  session's own `tokens_*` and `summary_*` columns. This is the only end-to-end
  correctness check any provider makes available and it should gate phase 1.
- **Ingest integration** — for JSONL: rotation, truncation, partial trailing
  line. For SQLite: cursor advance across `time_updated`, a busy database, and
  confirmation that WAL-resident rows are seen (the regression an `immutable=1`
  handle would introduce). For both: crash between fact write and cursor
  advance, and re-ingest idempotence.
- **Isolation** — ingest must never open a provider database writable; asserted
  by opening the fixture read-only and verifying no `-wal` growth.
- **Frontend unit** — panel rendering across empty, partial, and estimated-data
  states; horizon switching; formatting.
- **Browser E2E** — Dashboard and Analytics render against a seeded telemetry
  store under an isolated `WARDIAN_HOME`; drill-through navigation.
- **Native E2E** — ingest reads a real on-disk provider log through `invoke`,
  which the browser layer cannot prove.

Per the layer boundary rules, anything asserting real filesystem log reads
belongs in native E2E, not browser E2E.

## Consequences

- **Positive**: Dashboard gets a job that no other surface has. The overlap
  between Dashboard, Agents, and Roster resolves in favour of the one surface
  that was redundant.
- **Positive**: Historical telemetry becomes a queryable store rather than a
  forensic exercise, and `wardian-core` placement means the CLI inherits it.
- **Positive**: Parse-once with watermarks removes the reason the current code
  tail-bounds at 4 MiB, without reintroducing the cost that bound was avoiding.
- **Positive**: Rate limit headroom and cache-split token accounting are more
  actionable on subscription plans than a dollar estimate would be, which is
  fortunate given that cost coverage is too thin to display.
- **Positive**: OpenCode turns out to need almost no parsing and supplies
  measured durations, a five-way token split, and pre-aggregated edit counts. It
  becomes the reference against which the inferring providers are validated.
- **Negative**: Coverage is uneven and the UI has to keep saying so. Antigravity
  has no tokens and no line counts, so a "tokens by provider" chart is
  incomplete by construction rather than by omission. That gap is permanent
  until Antigravity documents its format, and corroborated by ccusage reaching
  the same conclusion.
- **Negative**: Four source implementations (opencode, codex, claude,
  antigravity) against four external formats Wardian does not control is ongoing
  maintenance, none of them formally documented, and format drift will be found
  by users before CI unless fixtures are refreshed.
- **Negative**: A new SQLite table set and a background ingest task add moving
  parts to startup and to the state layer.
- **Negative**: File and line metrics are structurally incomplete until phase 4,
  and possibly after it. Shell-based edits may never be attributable.
- **Negative**: Replacing `DashboardView` discards a surface some workflows may
  rely on. Agents overview and Roster cover the per-agent case, but the specific
  card layout goes away.

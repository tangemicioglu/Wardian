# Agent Change Review

## Decision

Wardian surfaces recent agent file changes through a Changes pane in the left
sidebar, built on primitives that already exist: conversation turn records for
attribution and time-slicing, and the working tree's own git object store for
content. No new storage subsystem ships in the first phase.

Runtime cost is the governing constraint. Every design choice below resolves in
favour of the cheaper option, and the optional snapshot layer in Phase 2 is
gated behind explicit measured budgets rather than shipped on intent.

Git is authoritative for *what* changed. Turn records are authoritative for
*who* changed it and *when*. Neither is treated as sufficient alone.

## Cost Basis

Measured on this repository (1426 tracked files, 24.1 MiB, NTFS), with
`node_modules`, `src-tauri/target`, and `dist` excluded by `.gitignore`:

| Operation | Cost |
| --- | --- |
| Cold directory walk, ignores applied, no hashing | 81 ms |
| Warm `add -A` with a persistent index, tree unchanged | 92 ms |
| `write-tree` | 85 ms |
| Full re-hash where objects already exist in the store | 862 ms |
| Full re-hash and loose-object write, warm page cache | 3056 ms |
| First-ever `add -A`, cold page cache | 27969 ms |
| One full-tree snapshot on disk | 12.7 MB |
| Incremental cost per changed snapshot | ~53 KB |

Two results drive the decision. The 862 ms figure shows snapshot cost collapses
when blobs already exist in the object store, which is true for every committed
file. The 27969 ms figure is a one-time cold read of the working tree, not an
intrinsic git cost, and it is avoided entirely by not maintaining a second
object store.

## Baselines

The pane exposes one axis of baselines. The current side of every comparison
is the live working tree.

| Mode | Baseline | Storage cost |
| --- | --- | --- |
| `last_effective_turn` | End of the most recent turn that changed a file | none in Phase 1 |
| `conversation_start` | First turn of the active conversation | none in Phase 1 |
| `branch_point` | `git merge-base` against the default branch | none |
| `head` | `HEAD` | none |
| `unreviewed` | Stored review watermark | watermark record only |

`branch_point` and `head` are computed from the working tree and cost nothing
beyond the diff itself. `last_effective_turn` and `conversation_start` resolve
to turn-record file lists in Phase 1 and to snapshot refs in Phase 2.

A turn that writes no files does not advance `last_effective_turn`. A read-only
analysis turn must not blank a diff the operator is still reading.

## Phase 1

### Placement

Changes is a left-sidebar pane, not a workbench surface. It takes its own
icon-rail entry beside Source Control and renders through
`SidebarContentPane.tsx`, mirroring `GitPanel` in structure: a scoped file list
above an inline diff.

Its chrome is the sidebar standard: a title, the baseline selector, and the
change set. No descriptive subtitle, and no manual refresh or review buttons.
Sidebar panes are glanceable and self-maintaining; explanatory prose and manual
controls are friction in a pane the operator checks dozens of times a session.

Reviewing changes is peripheral and glanceable work performed *while* watching
an agent. A workbench surface would consume a tab slot next to the agent session
it describes, and would place Changes in a different structural class from
Source Control, which is the same kind of object with a different baseline.

Changes does not live inside the Explorer pane. Explorer presents a
whole-workspace file tree; Changes presents a time-scoped, filtered change list
with baselines and attribution. One pane cannot carry both data shapes without a
mode toggle.

Changes does not extend `GitPanel`. Source control semantics and agent-turn
semantics stay in separate modules.

### Composition

The pane composes existing modules and adds no backend subsystem:

- `ConversationTurnRecord` in `crates/wardian-core/src/conversations.rs`
  supplies `turn_index`, `started_at`, `files.written`, `tools_used`, and
  `external_side_effects` paths.
- `git_status`, `git_diff_file`, `git_diff_file_against_workspace`, and
  `git_show_file_revision` in `src-tauri/src/commands/git.rs` supply change sets
  and diff content.
- `fileDiffModel.ts` and `FileComparisonLens.tsx` render the diffs.

### Invalidation

The pane refreshes itself. It carries no manual refresh control.

Three signals drive recomputation, and between them the cached change set is
served without any git invocation:

- `explorer-changed` — the working-tree watcher behind `explorer_watch` in
  `src-tauri/src/commands/fs.rs`. This is the primary signal, and the only one
  that observes agent writes, whether through an edit tool or a shell.
- turn completion — retained so attribution refreshes when a turn closes, even
  if the tree did not change in the same window.
- `git-changed` — the operator's own staging, commits, and branch switches.

`git_watch` observes only `.git/index` and `.git/HEAD`, so `git-changed` alone
can never see the writes this pane exists to show. It stays subscribed for
operator git actions and must not be relied on for agent writes.

`explorer_watch` already debounces at 150 ms and excludes `.git`,
`node_modules`, `target`, `.venv`, `dist`, `build`, `.next`, `.turbo`, and
`.cache`. The pane inherits that filtering rather than reimplementing it, and
must coalesce bursts: an agent writing fifty files produces one recomputation,
not fifty.

The pane must not imitate `ExplorerPanel`'s three-second `git_status` poll. That
interval exists to paint status badges on a file tree and is precisely the
per-render cost these rules forbid.

A turn in flight now updates the change set as its writes land. Mid-turn
updating is a consequence of watching the working tree, not a separate feature.

### Cost rules

These rules are binding, not advisory.

The pane must not poll `git_status` on a timer, and must not scan the working
tree on render.

Diff content is fetched per file on expansion, never eagerly for the whole
change set. A change set of 200 files costs one status call, not 200 diff calls.

Line counts for the whole change set are obtained in a single `git diff
--numstat` invocation, never per file. No such command exists in
`src-tauri/src/commands/git.rs` today; Phase 1 adds exactly one.

Turn records are read from the conversation archive, which is already
materialised on disk. The pane must not re-derive turns from provider
transcripts.

The turn read is **bounded**, not whole-archive. Every baseline the pane offers
is recent: the last effective turn, the active conversation, a branch point, a
commit, or the operator's last look. None requires months of history. Reading
every conversation in a workspace makes recomputation cost grow with total
archive size forever, which no baseline justifies.

The pane reads the active conversation, plus at most a bounded window of the
most recently updated conversations for the workspace. A path whose writing turn
falls outside that window presents as `inferred` rather than `attributed`, which
is the same graceful degradation a skipped record produces.

### Archive resilience

A turn record the pane cannot parse is **skipped, not fatal.** The archive spans
years of schema drift, so even a bounded read will encounter records written by
older writers. One unparseable record must never blank the change set.

Git remains the source of the change set, so degraded attribution is a partial
loss, not a failure: skipped records cost `attributed` evidence on their paths,
which then present as `inferred`. The pane never surfaces a raw deserialization
error as its only content.

Skipped records are counted but **not reported as a routine warning.** Measured
on this archive, 7381 of 12381 turn records predate the current schema and
cannot be parsed at all, missing nine required fields rather than merely
carrying a null. For one agent in this workspace the ratio is 3201 legacy to 255
current. A visible count in that range reads as breakage while describing
ordinary history. The count belongs in diagnostics; the pane stays quiet unless
attribution is degraded for a path the operator is actually looking at.

Legacy-shaped records are not rehabilitated. Supporting a schema that lacks
`conversation_id`, `turn_index`, `turn_key`, `started_at`, `updated_at`,
`request`, `counts`, `files`, and `record_refs` would mean maintaining a second
record shape to recover attribution for conversations months old, which no
baseline in this pane looks at. The bounded read keeps them out of the hot path
instead.

Legacy records may carry `null` for fields later typed as required. Reading
those fields must tolerate an explicit `null` and fall back to a default, not
merely a missing key: `#[serde(default)]` alone does not cover a present-but-null
value.

### Attribution

Every entry carries an `evidence` discriminant:

- `attributed` — the path appears in some turn's `files.written` or
  `external_side_effects`, so it maps to a specific agent and turn.
- `inferred` — git reports the path as changed but no turn record claims it.

`files.written` is populated from edit-tool events (`edit`, `write`,
`multiedit`, `notebookedit`, `write_to_file`, `replace_file_content`,
`multi_replace_file_content`) and from `apply_patch` path extraction. Shell
commands are not parsed, so writes performed through a terminal produce
`inferred` entries. `files.written` is therefore a lower bound and is never
used to *filter* the change set, only to annotate it.

### Known limits

Phase 1 cannot reconstruct the content a past turn produced once the file has
changed again, and cannot restore prior state. It degrades gracefully: the
operator still sees which files that turn touched and their current content.

## Phase 2

Phase 2 adds per-turn content snapshots. It ships only if it meets the budgets
below under measurement; it does not ship on the strength of this document.

### Placement

Snapshots are parentless commits written into the **working tree's own object
store** under `refs/wardian/<agent_id>/<conversation_id>/<turn_index>`, using a
dedicated index file. HEAD, the operator's index, and all branches are never
modified.

Blobs dedup against existing history, so the storage floor is approximately
zero and only uncommitted content is new. Parentless refs are independently
deletable, so retention is a ref drop followed by garbage collection, with no
history rewriting. Refs under this namespace are absent from `git branch`, and
from default push and fetch refspecs.

A separate shadow repository is explicitly rejected. It costs a 12.7 MB floor
per workspace and a multi-second first snapshot, buys nothing that a private
ref namespace does not, and is opaque to the operator's own tooling.

### Budgets

| Constraint | Limit |
| --- | --- |
| Turn-boundary snapshot, p95 | ≤ 250 ms |
| First snapshot in a repository with committed history | ≤ 2 s |
| Added storage floor per workspace | ~0 |
| Turn-boundary work on the agent's critical path | none |

Snapshots run asynchronously off the turn-boundary event. A snapshot that
exceeds its budget is abandoned, not queued.

### Required optimisations

The per-workspace index file is persistent. Its loss re-incurs the cold cost and
it must survive restarts.

A turn whose `files.written` is empty and whose `tools_used` contains no shell
tool is skipped without a tree walk.

A snapshot whose `write-tree` yields the tree already referenced by the previous
snapshot creates no new ref; the existing commit is reused. This is the reliable
no-op check, because `git_watch` does not observe working-tree writes.

`core.untrackedCache` and `core.preloadIndex` are enabled on the index. Working
trees beyond roughly 10,000 tracked files additionally require `core.fsmonitor`.

Garbage collection runs only on idle, under a lock, never during a turn.

### Retention

Retention is bounded by policy, not by turn count:

- Active conversation: every effective turn, capped at a rolling window.
- Closed conversation: first and last effective turn only.
- Archived agent or expired horizon: final snapshot only.
- Per-agent byte budget with least-recently-used eviction.

A rolling window of 20 turns costs approximately 1 MB, which is why the window
exists rather than a strict fixed set of baselines.

Pinned baselines are the unbounded risk, not turn count. A long-lived
`conversation_start` ref holds every superseded blob for the life of the
conversation. When a pinned baseline diverges past a configured threshold the
pane warns and offers to re-anchor it.

## Data Model

Properties are `snake_case` in both Rust and TypeScript.

```
ChangeReviewBaseline =
  | last_effective_turn
  | conversation_start
  | branch_point
  | head
  | unreviewed

ChangeReviewEvidence = attributed | inferred

ChangeReviewChangeKind = added | modified | deleted | renamed | untracked

ChangeReviewFileEntry {
  path: String
  change_kind: ChangeReviewChangeKind
  old_path: Option<String>
  insertions: Option<u64>
  deletions: Option<u64>
  evidence: ChangeReviewEvidence
  agent_ids: Vec<String>
  turn_indices: Vec<u64>
  binary: bool
  truncated: bool
  reviewed: bool
}

ChangeReviewSummary {
  schema: u8
  baseline: ChangeReviewBaseline
  baseline_ref: Option<String>
  from_turn_index: Option<u64>
  to_turn_index: Option<u64>
  files: Vec<ChangeReviewFileEntry>
  computed_at: String
  truncated: bool
}

ChangeReviewReviewedPath {
  path: String
  change_kind: ChangeReviewChangeKind
  insertions: Option<u64>
  deletions: Option<u64>
}

ChangeReviewWatermark {
  schema: u8
  agent_id: String
  workspace: String
  reviewed_turn_index: u64
  reviewed_at: String
  reviewed_head: Option<String>
  reviewed_paths: Vec<ChangeReviewReviewedPath>
}
```

Review state advances by **expanding a file's diff**, not by a button. Expanding
is the act of reviewing, so the record follows the operator's actual attention
per path rather than declaring a whole batch reviewed at once. The pane writes
the watermark on expand and at no other time.

The `unreviewed` baseline **never removes a path that git currently reports as
changed.** It marks entries: an entry whose path and numstat signature match a
`reviewed_paths` record is `reviewed`, and everything else is not. Presentation
may collapse or dim reviewed entries; the change set itself stays complete.

A turn-index watermark alone cannot carry this baseline. `latest_effective_turn`
advances only on a `files.written` or `external_side_effects` claim, so a
shell-driven write does not move it. Clearing the change set on a turn-index
comparison therefore hides exactly the writes that `evidence: inferred` exists
to surface, which is the annotate-never-filter rule inverted. The signature is
already computed for every entry, so per-path comparison costs nothing
additional.

A file edited back to its reviewed signature reads as reviewed. That is
accepted: Phase 1 has no content baseline to distinguish it, and the failure is
conservative in the safe direction.

```
ChangeReviewPrefs {
  schema: u8
  baseline: ChangeReviewBaseline
}
```

The pane unmounts when the sidebar tab changes, as `GitPanel` does, so the
selected baseline is persisted rather than held in component state. It lives at
`WARDIAN_HOME/changes/prefs.json`, following `load_watchlist_prefs` and
`save_watchlist_prefs` in `src-tauri/src/commands/watchlist.rs`. A missing or
unparseable file yields the default baseline, `last_effective_turn`.

The preference is global rather than per agent. Operators review with a
habitual baseline, and a per-agent record would cost a write on every agent
switch to serve a distinction nobody asked for.

Watermarks persist as a single index at `WARDIAN_HOME/changes/watermarks.json`,
keyed by `agent_id` and `workspace`, following the precedent set by
`watchlists/index.json` in `src-tauri/src/commands/watchlist.rs`. They are not
written into agent workspace directories, which belong to the agent rather than
to Wardian, and they are not added to `settings/state.json`.

The directory is created on first write. A missing or unparseable index is
treated as "nothing reviewed yet" rather than an error, so a corrupt file
degrades the `unreviewed` baseline instead of breaking the pane.

The watermark index and the preferences file are the only new persisted records
in Phase 1. Both live under `WARDIAN_HOME/changes/`.

## Edge Cases

A workspace that is not a git repository yields turn-record file lists with no
diff content. The pane states this rather than rendering an empty change set.

Several agents sharing one workspace produce a single git-derived change set
with per-entry `agent_ids`. Attribution is an overlay; the change set is never
partitioned by agent, because git cannot support that partition.

A detached HEAD, or a repository with no default branch, degrades
`branch_point` to `head`.

A rebase or amend can orphan `reviewed_head`. The watermark then falls back to
`reviewed_turn_index` alone, and the pane marks the comparison approximate.

Binary files and files beyond the size cap are listed with `binary` or
`truncated` set and no rendered content.

Untracked files are included, subject to `.gitignore`. Ignore rules are
load-bearing: a workspace that snapshots `node_modules` churns hundreds of
megabytes on a single install.

An archive containing records the pane cannot parse still yields a change set.
The unparseable records are skipped and counted; their paths lose `attributed`
evidence and present as `inferred`.

Path identity is compared case-insensitively only under the Windows target
configuration. POSIX and macOS comparisons preserve case, so two workspaces or
claimed paths differing only by case stay distinct.

Deleting an agent removes its refs under `refs/wardian/<agent_id>/` and its
watermark.

## Verification

- The Changes pane issues no git invocation between recomputation triggers.
- A file written by an agent appears in the change set after the turn ends,
  without any `git-changed` event having fired.
- Line counts for a change set of any size cost one `git diff --numstat`
  invocation.
- Expanding a file fetches exactly one diff; collapsing and re-expanding within
  a change set fetches none.
- A turn that writes no file leaves the `last_effective_turn` baseline and its
  rendered diff unchanged.
- A file written through a shell command appears in the change set with
  `evidence: inferred` and empty `agent_ids`.
- A file written through an edit tool appears with `evidence: attributed` and
  the originating `agent_id` and `turn_index`.
- Marking reviewed advances the watermark, and the `unreviewed` baseline then
  reports an empty change set until the next effective turn.
- A non-git workspace renders file lists and an explicit no-diff-content state.
- After marking reviewed, a path written through a shell remains present under
  the `unreviewed` baseline with `evidence: inferred`, empty `agent_ids`, and
  `reviewed: false`. No baseline ever empties a non-empty git change set.
- Workspace and path identity comparisons are case-insensitive only on Windows.
- An agent write refreshes the pane with no operator action and no `git-changed`
  event, through `explorer-changed` alone.
- A burst of writes across many files produces one recomputation, not one per
  file.
- Expanding a file marks that path reviewed; the next load under the
  `unreviewed` baseline omits it while leaving unexpanded paths present.
- The pane renders no refresh control, no review control, and no subtitle.
- A `turns.jsonl` containing a record with `"status_source": null` loads, and the
  pane renders a change set rather than a deserialization error.
- A single unparseable turn record does not empty the change set.
- An archive whose records are majority legacy-shaped yields a working pane with
  no visible warning, and recomputation cost does not scale with total archive
  size.
- Phase 2 only: a turn-boundary snapshot completes within budget, leaves HEAD,
  the operator's index, and all branches unmodified, and adds no ref visible to
  `git branch`.

## Non-Goals

Restoring or rewinding files to a prior turn. The Phase 2 ref namespace makes it
possible later; this specification does not define it.

Hunk-level accept and reject of agent changes. Wardian reviews changes already
written to disk; it does not gate them.

Parsing shell commands to attribute terminal-driven writes. Git already reports
those changes, and `inferred` is the honest label.

A separate shadow object store for non-git workspaces. Rejected on cost until a
measured need exists.

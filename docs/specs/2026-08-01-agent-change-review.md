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

The change set is recomputed on **turn boundaries**, not on file events.

`git_watch` watches only `.git/index` and `.git/HEAD`. An agent writing a file
through an edit tool or a shell touches neither, so `git-changed` does not fire
for the changes this pane exists to show. It is subscribed to as a secondary
signal, covering the operator's own staging, commits, and branch switches, and
must not be relied on for agent writes.

Recomputation is therefore triggered by: a turn reaching an effective end, a
`git-changed` event, a baseline change, and explicit operator refresh. Between
those, the cached change set is served without any git invocation.

Live mid-turn updating is a non-goal. A turn in flight shows the change set as
of its start until it ends.

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

ChangeReviewWatermark {
  schema: u8
  agent_id: String
  workspace: String
  reviewed_turn_index: u64
  reviewed_at: String
  reviewed_head: Option<String>
}
```

Watermarks persist as a single index at `WARDIAN_HOME/changes/watermarks.json`,
keyed by `agent_id` and `workspace`, following the precedent set by
`watchlists/index.json` in `src-tauri/src/commands/watchlist.rs`. They are not
written into agent workspace directories, which belong to the agent rather than
to Wardian, and they are not added to `settings/state.json`.

The directory is created on first write. A missing or unparseable index is
treated as "nothing reviewed yet" rather than an error, so a corrupt file
degrades the `unreviewed` baseline instead of breaking the pane.

This is the only new persisted record in Phase 1.

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

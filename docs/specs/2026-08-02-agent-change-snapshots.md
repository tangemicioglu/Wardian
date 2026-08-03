# Agent Change Snapshots (Phase 2)

Phase 2 of [Agent Change Review](2026-08-01-agent-change-review.md). Phase 1
shipped in PR #809 and compares the live working tree against baselines it can
compute for free. It cannot reconstruct the content a past turn produced once
the file changed again.

Phase 2 adds per-turn content snapshots so `last_effective_turn` and
`conversation_start` resolve to real content rather than to a file list, and so
`unreviewed` compares content rather than a numstat signature.

Phase 1 required that Phase 2 "ships only if it meets the budgets below under
measurement". This document reports that measurement. Phase 2 **passes on cost**,
and **one of its stated budgets was wrong** and is replaced below.

## Gate Result

Measured on this repository (1466 tracked files, NTFS, Windows 11), snapshotting
into the working tree's own object store with a dedicated index file.

| Operation | Cost |
| --- | --- |
| Bare `git` process spawn (`rev-parse HEAD`) | 56–68 ms, 305 ms outlier |
| `add -A` into a **fresh** dedicated index | **85582 ms** |
| `add -A` into an index **seeded from `.git/index`** | **78 ms** |
| `write-tree`, first call | 528 ms |
| `commit-tree` | 184 ms |
| `update-ref` | 62 ms |
| No-change snapshot (`add -A` + `write-tree`), median of 10 | 115 ms |
| One-file-changed full snapshot, median of 10 | 476 ms |
| One-file-changed full snapshot, p95 of 10 | **~860 ms** |
| Full first snapshot, seeded (add + write-tree + commit-tree + update-ref) | 858 ms |
| Dedicated index file size | 173 KB |

Two results decide the design.

**The seeded index is load-bearing, at a factor of 1097.** A fresh dedicated
index re-hashes the entire working tree: 85.6 seconds. An index byte-copied from
the operator's own `.git/index` carries valid stat data for every tracked file,
so `add -A` hashes only what actually changed: 78 ms. This is not an
optimisation. A Phase 2 that creates its index with `git read-tree` or from
empty is unshippable, and the failure mode is a minute and a half of disk thrash
on first use.

**Process spawn dominates everything else.** A no-change snapshot costs 115 ms
across two spawns whose floor is ~120 ms; the index and tree work is
approximately free. A full snapshot is four spawns, so ~240 ms is pure process
creation before any work happens.

### The 250 ms budget was unachievable and is withdrawn

Phase 1 set "turn-boundary snapshot, p95 ≤ 250 ms". That number was written
without measurement. On Windows the four-spawn floor alone is ~240 ms, so the
budget mandated in-process libgit2 by accident rather than by decision.

Adding `git2` to satisfy a number I invented is the wrong trade. It introduces a
vendored C dependency, a new build and security-audit surface, and a second git
implementation alongside 3692 lines of `src-tauri/src/commands/git.rs` that all
shell out through one helper at `git.rs:160`.

The budget is re-derived from what it protects. Snapshots are asynchronous and
off the agent's critical path, so latency is not what guards the agent —
asynchrony is. The real constraints are that a snapshot must not delay the
agent, must not still be running when the next turn boundary arrives, and must
not thrash disk. Turn boundaries are seconds to minutes apart; 860 ms clears
that by an order of magnitude.

| Constraint | Limit | Measured |
| --- | --- | --- |
| Turn-boundary snapshot, p95 | ≤ 1 s | ~860 ms |
| Turn-boundary snapshot, hard abandon | 5 s | — |
| First snapshot in a repository with committed history | ≤ 2 s | 858 ms |
| Added storage floor per workspace | ~0 | ~0 |
| Turn-boundary work on the agent's critical path | none | none |
| New native dependencies | none | none |

A snapshot that exceeds the hard abandon limit is dropped, not queued.

If a workspace is later measured to miss the 1 s p95 — very large trees, or a
host where spawn cost is pathological — the response is `core.fsmonitor` and
spawn reduction first, and libgit2 only with its own measurement behind it.

### Environmental caveat

Timings show recurring outliers (a 305 ms bare `rev-parse`, a 357 ms no-change
snapshot) against stable ~60 ms and ~115 ms medians. These are consistent with
on-access antivirus scanning of newly written objects, which is the ordinary
condition on Windows developer machines. The budgets above are stated at p95
precisely so this behaviour is inside the budget rather than an excuse for
missing it.

## Resolved Design Questions

Phase 1 left two questions open. Both are resolved here.

### The snapshot index is per workspace, not per agent

A working tree is one filesystem. When several agents share a workspace, no
index arrangement can produce a tree that reflects one agent's writes and not
another's, because the content on disk is already interleaved. Phase 1 states
this for attribution: "the change set is never partitioned by agent, because git
cannot support that partition."

A per-agent index would imply a per-agent content timeline that git cannot
deliver, and would multiply hashing work by the number of agents in the
workspace to produce N copies of the same tree.

Therefore: **one index per workspace**, at
`WARDIAN_HOME/changes/indices/<workspace_hash>.index`, guarded by a mutex held
only for the snapshot itself. Contention is nil in the common case of one agent
per workspace, and correct in the shared case, where two concurrent turn
boundaries collapse into one snapshot of the one true tree.

Refs stay keyed by agent, so attribution remains an overlay on shared content —
the same architecture as Phase 1.

### Pinned-baseline divergence is measured in turns and paths, not bytes

A long-lived `conversation_start` ref holds every superseded blob for the life of
the conversation. This is the only unbounded cost in the design.

The intuitive threshold is bytes uniquely held by the pin, and it is the wrong
one: computing it requires `git rev-list --objects` plus `cat-file --batch-check`
over the pinned commit, which is a repository-wide walk run to decide whether to
show a warning. That inverts the governing constraint of this feature.

The threshold is therefore a proxy already tracked at zero additional cost: a
pinned baseline is **diverged** when more than **100 effective turns** or more
than **200 distinct changed paths** have accumulated since the pin. Both counters
are byproducts of work the pane already performs. At the measured ~53 KB
incremental cost per changed snapshot, 100 turns is on the order of 5 MB held —
comfortable, and an honest place to prompt rather than to enforce.

On divergence the pane warns and offers to re-anchor. It does not silently
re-anchor: the pin is the operator's explicit choice of what to compare against,
and moving it without consent destroys the comparison they asked for.

## Placement

Unchanged from Phase 1, and restated because it is a safety boundary.

Snapshots are parentless commits in the **working tree's own object store**,
under `refs/wardian/<agent_id>/<conversation_id>/<turn_index>`, written through a
dedicated index file. HEAD, the operator's index, and all branches are never
modified. Blobs dedup against existing history, so the storage floor is
approximately zero. Refs in this namespace are absent from `git branch` and from
default push and fetch refspecs, and are independently deletable, so retention is
a ref drop plus garbage collection with no history rewriting.

A separate shadow repository stays rejected: a 12.7 MB floor per workspace, a
multi-second first snapshot, and opacity to the operator's own tooling, for
nothing a private ref namespace does not already provide.

## Required Behaviour

**The dedicated index is seeded by byte-copying `.git/index`** on creation, and
is persistent thereafter. If it is missing, unreadable, or its git index version
is unrecognised, it is re-seeded from `.git/index`. It is never created empty and
never initialised with `read-tree`. Loss of the file costs one re-seed, not a
re-hash.

**A turn with no writes is skipped without a tree walk.** A turn whose
`files.written` is empty and whose `tools_used` contains no shell tool produces
no snapshot and no `add -A`.

**An unchanged tree creates no ref.** When `write-tree` yields the tree already
referenced by the previous snapshot, the existing commit is reused. This is the
reliable no-op check, because `git_watch` does not observe working-tree writes.
It costs the 115 ms no-change path, and it is why that path was measured.

**Snapshots coalesce; they do not queue.** If a snapshot is in flight when a turn
boundary fires, the workspace is marked dirty and exactly one snapshot runs on
completion. Intermediate boundaries are dropped, and the resulting snapshot is
attributed to the most recent effective turn.

**`core.untrackedCache` and `core.preloadIndex` are set on the dedicated index.**
Working trees beyond roughly 10,000 tracked files additionally require
`core.fsmonitor`.

**Garbage collection runs only on idle, under the workspace lock, never during a
turn.**

**Ignore rules are load-bearing.** A snapshot that captures `node_modules`
churns hundreds of megabytes on a single install. The dedicated index inherits
the repository's ignore configuration; it does not relax it.

## Retention

Bounded by policy, not by turn count:

- Active conversation: every effective turn, capped at a rolling window of 20.
- Closed conversation: first and last effective turn only.
- Archived agent or expired horizon: final snapshot only.
- Per-agent byte budget with least-recently-used eviction.

A rolling window of 20 turns costs approximately 1 MB.

Deleting an agent removes `refs/wardian/<agent_id>/` and its watermark, as in
Phase 1.

## Data Model

Properties are `snake_case` in both Rust and TypeScript, extending the Phase 1
model in `src-tauri/src/commands/change_review.rs`.

```
ChangeSnapshotRef {
  agent_id: String
  conversation_id: String
  turn_index: u64
  commit_id: String
  tree_id: String
  created_at: String
  effective: bool
}

ChangeSnapshotIndex {
  schema: u8
  workspace: String
  index_path: String
  seeded_from_head: bool
  snapshots: Vec<ChangeSnapshotRef>
  last_tree_id: Option<String>
}

ChangeSnapshotPin {
  schema: u8
  agent_id: String
  workspace: String
  baseline: ChangeReviewBaseline
  pinned_commit: String
  pinned_at: String
  turns_since_pin: u64
  paths_since_pin: u64
  diverged: bool
}
```

`ChangeReviewBaseline` is unchanged. Phase 2 changes how two of its variants
resolve, not what the operator selects: `last_effective_turn` and
`conversation_start` resolve to a snapshot commit when one exists, and fall back
to the Phase 1 turn-record file list when one does not. The fallback is
permanent, not transitional — a workspace that is not a git repository never
gets snapshots.

`unreviewed` gains a content anchor. Phase 1 accepted that "a file edited back to
its reviewed signature reads as reviewed" because it had no content baseline.
With a snapshot at the review point, that limitation is removed.

The annotate-never-filter rule survives intact: a snapshot is a better baseline,
never a filter. **No snapshot state may remove a path that git currently reports
as changed.** This is the invariant that Phase 1's autoreview caught being
violated, and it is restated here because Phase 2 introduces a second way to
violate it.

## Edge Cases

A workspace that is not a git repository takes no snapshots and keeps Phase 1
behaviour.

A repository with no commits has no `.git/index` to seed from, so the first
snapshot pays the cold cost. It is taken once, asynchronously, and the pane
reports Phase 1 behaviour until it completes.

A concurrent operator `git` command holding `.git/index.lock` does not block a
snapshot, because the snapshot writes a different index file. An operator commit
mid-snapshot yields a snapshot of the tree as it was read; it is a content
timeline, not a transaction against HEAD.

A rebase, amend, or `gc --prune` by the operator can orphan a snapshot commit. A
ref whose commit no longer resolves is dropped and the baseline degrades to
Phase 1, as with an orphaned `reviewed_head`.

Concurrent turn boundaries from several agents in one workspace produce one
snapshot under the lock, referenced by each agent's ref.

Submodules are not traversed. `write-tree` records the gitlink, matching what
`git status` reports for the pane.

A snapshot that fails for any reason is logged and dropped. Phase 2 never
converts a snapshot failure into a pane failure.

## Verification

- A first snapshot in a repository with committed history completes within 2 s,
  and the dedicated index is byte-identical in provenance to `.git/index` at
  creation.
- A snapshot leaves HEAD, `.git/index`, and all branches unmodified, and adds no
  ref visible to `git branch`.
- A turn-boundary snapshot completes at p95 ≤ 1 s across at least 10 runs.
- A turn whose tree is unchanged creates no new ref and reuses the previous
  commit.
- A turn with no writes and no shell tool triggers no `add -A`.
- A snapshot in flight during three further turn boundaries results in exactly
  two snapshots total, the second attributed to the most recent effective turn.
- Deleting the dedicated index file causes a re-seed, not a full re-hash: the
  next snapshot stays within the first-snapshot budget.
- A file edited and then reverted to its reviewed content reads as **not**
  changed under `unreviewed`, which Phase 1 could not do.
- No snapshot state removes a path that git reports as changed, under any
  baseline.
- A pinned baseline past 100 turns or 200 paths reports `diverged` and is not
  silently re-anchored.
- Deleting an agent removes `refs/wardian/<agent_id>/` and leaves other agents'
  refs intact.
- An orphaned snapshot commit degrades the baseline to Phase 1 rather than
  erroring.
- Snapshot cost does not scale with total archive size.

## Implementation Plan

Ordered so each step is independently verifiable, and so the risky part is
proven before anything depends on it.

1. **Snapshot primitive**, `src-tauri/src/commands/change_snapshot.rs`. Index
   seeding by byte-copy, `add -A`, `write-tree`, `commit-tree`, `update-ref`,
   through the existing `run_git` helper. Unit tests for the seed path, the
   unchanged-tree no-op, and the HEAD/index/branch non-mutation invariant.
2. **Benchmark test** asserting the first-snapshot and p95 budgets on a fixture
   repository. This is the gate; it belongs in the suite, not in a document.
3. **Workspace lock and coalescing**, in `state/`, using the existing
   `tokio::sync::Mutex` convention. Test that three boundaries during one
   in-flight snapshot yield exactly two snapshots.
4. **Turn-boundary hook**, asynchronous, off the critical path, with the
   no-write skip.
5. **Baseline resolution** in `change_review.rs`: resolve to a snapshot commit
   when present, fall back to Phase 1 otherwise. The annotate-never-filter
   invariant gets an explicit test here.
6. **Retention and garbage collection**, idle-only, under the lock.
7. **Divergence counters and the re-anchor prompt**, backend counters first,
   then the pane affordance.
8. **Frontend**: no new pane. `unreviewed` becomes content-accurate and pinned
   baselines can warn. Screenshot evidence covers the re-anchor prompt, per the
   PR screenshot gate.

Steps 1 and 2 are the decision point. If the benchmark test cannot hold its
budget on CI hardware, stop and re-measure before building anything on top of it.

## Non-Goals

Restoring or rewinding files to a prior turn. The ref namespace makes it
possible; this document does not define it.

Hunk-level accept and reject. Wardian reviews changes already on disk.

Pushing or sharing snapshot refs. The namespace is local and excluded from
default refspecs deliberately.

An in-process libgit2 backend. Reconsidered only with measurement showing the
shell-out path missing its budget on real hardware.

A shadow object store for non-git workspaces. Those keep Phase 1 behaviour.

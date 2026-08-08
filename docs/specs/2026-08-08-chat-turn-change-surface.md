# Chat Turn Change Surface

Filename: `2026-08-08-chat-turn-change-surface.md`

- **Status:** Implemented (partial — see Delivery status)
- **Date:** 2026-08-08

## Delivery status

Shipped on `feat/chat-turn-change-surface`:

| Area | Outcome |
|---|---|
| Shared presentation layer | `features/chat/` now owns transcript classification and rows; desktop and remote both consume it. 1190 lines of duplication removed. |
| Structured edits | `structuredEdit.ts` recovers `Edit`/`MultiEdit`/`Write` changes from `metadata.tool_input` into a Before/after panel. |
| Turn segmentation | `chatTurns.ts` splits on user messages and emits a `turn_change_summary` row per turn that touched files. |
| Turn change card | `TurnChangeCard.tsx` with scope-aware preview, `DiffStat`, auto-expand, and workbench navigation on desktop. |
| Correctness | Heading/summary de-duplication; in-flight tone demoted once the agent stops; only the pending approval stays actionable. |
| Work log | Groups at 3, previews 2, and reports elapsed time when the provider timestamped the events. |
| Provider conformance | Every feature above checked against each provider's real event shape; `providerConformance.test.ts` pins the result. |

## Provider coverage

The features above were designed against Claude-shaped events, where the
change lives in `metadata.tool_input`. No other provider emits that shape, so
each was checked against what `providers/chat_transcript.rs` actually produces.

| Provider | Where a change is reported | Turn card outcome |
|---|---|---|
| Claude | `metadata.tool_input` (`old_string`/`new_string`, `content`) | Exact counts |
| OpenCode | `part.input` (`filePath`/`oldString`/`newString`) | Exact counts |
| Codex | `metadata.tool_input_text` — an `apply_patch` payload | Exact counts, per file |
| Antigravity | `files_written` only; args name the file, never the content | Path with unknown counts |
| Gemini | Nothing; `tool_use` carries a name and no arguments | No row |

Three defects surfaced from that check and are fixed:

- OpenCode's normalizer discarded a tool call's `input` outright, so its edits
  could not be recovered at any layer above.
- Codex's `apply_patch` is that provider's entire edit path and carries no
  event text. The patch sat unread in `tool_input_text` while the row rendered
  the bare word "Running".
- Whole-file creates were detected only through `metadata.tool_name`, which
  Claude, Gemini, and OpenCode never set. Claude's `Write` therefore produced
  no panel despite carrying the complete file.

Two behaviours remain provider-dependent by nature and are documented rather
than papered over:

- **Gemini reports no file writes at all.** The conformance suite asserts the
  absence, so a normalizer change that starts supplying paths fails a test and
  gets noticed instead of silently half-working.
- **Work-group duration is effectively Antigravity-only.** `created_at` is set
  by `status_event` and Antigravity's tool results; `message_event` and
  `tool_call_event` never set it. The label already returns null rather than
  estimating, so every other provider simply omits it.

**Deferred, with reasons:**

- **Git-verified turn changes.** The card is fed by transcript evidence, not
  `load_change_review`. Joining a chat turn to a backend turn record index
  cannot be validated offline, and a wrong mapping would attribute one turn's
  changes to another — a failure mode worse than the gap it closes. The card
  states its provenance rather than implying a working-tree diff.
- **Cross-turn folding.** `work_group` already owns a collapse affordance. A
  second fold spanning whole turns has to interleave with the view's
  `visibleRowLimit` pagination and `stickToLatest` scroll anchoring, neither of
  which can be exercised without a real app run. Work-log compression addresses
  the same noise inside the existing structure.
- **Scroll anchoring modes.** Unchanged; belongs with turn folding.
- **PR screenshots.** Browser E2E cannot seed a chat transcript — it arrives
  over `load_agent_chat_transcript` IPC — so evidence for these rows needs the
  native harness.

## Context and Problem Statement

`AgentChatView` renders a flat, chronological stream of `AgentChatEvent`s. Every
tool call becomes a row; a run of four or more consecutive work events collapses
into a `work_group` showing the latest six. The operator reads a dense ledger of
*what the agent did* and never a statement of *what the agent changed*.

Three concrete gaps, verified against the current code:

1. **No turn structure.** `derivePresentedChatRows` groups by adjacency of work
   events, never by `turn_id` (`src/features/grid/workLogPresentation.ts:25`).
   `turn_id` is consumed only to pair a `tool_call` with its `tool_result`
   (`providerLinkKeys`, line 272). A turn — the unit an operator actually
   reviews, revisits, and reverts — has no representation in the chat.

2. **Path strings stand in for changes.** `ChangedFiles`
   (`src/features/grid/AgentChatView.tsx:526`) renders bare path chips harvested
   heuristically from `event.path` and a metadata key allowlist
   (`extractMetadataPaths`, `workLogPresentation.ts:295`). No change kind, no
   `+/-` counts, no click target. `looksLikePath` (line 328) guesses by
   punctuation, so a read is indistinguishable from a write and a matched
   non-path string becomes a phantom "changed file".

3. **Real change data exists but never reaches the chat.** The backend already
   computes git-truth change attribution with per-file turn indices:
   `ChangeReviewFileEntry { path, change_kind, old_path, insertions, deletions,
   evidence, agent_ids, turn_indices, binary, reviewed }`
   (`src/types/index.ts:995`, produced by
   `src-tauri/src/commands/change_review.rs`). That data is consumed by exactly
   one surface — `ChangesPanel`, a right-rail pane scoped to a whole baseline
   window (`src/features/changes/ChangesPanel.tsx`). The chat, where the
   operator is actually reading, gets none of it.

The inline diff path is also weaker than it looks. `ToolBody`'s `diff`
presentation (`AgentChatView.tsx:709`) only triggers when the provider happens
to emit unified-diff *text*, and `diffStats` counts `+`/`-` line prefixes out of
that text. For Claude's `Edit` tool the provider emits no diff text at all — the
backend stores structured input instead:
`event.metadata.tool_input.{file_path, old_string, new_string}`
(`src-tauri/src/providers/chat_transcript.rs:482-492`), plus
`metadata.files_written` / `files_read` derived from the tool name
(`claude_tool_writes_file`). The UI reads `file_path` for a chip and discards
`old_string`/`new_string`. **The material for a real inline diff is already in
the frontend's hands and is being thrown away.**

### Prior art

Grounded against the current implementations rather than recollection.

**T3 Code** (`pingdotgg/t3code`, open source, same "bring your own subscription"
model as Wardian) is the closest analogue and the strongest reference:

- Its timeline row union is explicitly turn-aware:
  `work | work-toggle | turn-fold | message | proposed-plan | turn-plan |
  working` (`apps/web/src/components/chat/MessagesTimeline.logic.ts`).
- `MAX_VISIBLE_WORK_LOG_ENTRIES = 1`. Tool calls collapse to the **single**
  latest entry behind a `work-toggle` row carrying `hiddenCount`. Wardian shows
  six and only groups at four or more.
- The **assistant message row carries `assistantTurnDiffSummary`** and
  `revertTurnCount`. File-change information is a property of the turn's answer,
  not a decoration on individual tool calls.
- `ChangedFilesCard` (`apps/web/src/components/chat/ChangedFilesTree.tsx`)
  renders under the assistant message: a collapsed header reading
  `N changed files +X -Y`, expanding to a path-compacted directory tree
  (`buildTurnDiffTree`, single-child directories collapsed into `a/b/c`), each
  row clicking through to open that file's diff in the right panel.
- `DiffStatLabel` is a shared primitive with an `aria-label` of
  `"N additions, M deletions"` and compact `1.2k` formatting.
- Diffs render in a dedicated right-panel surface (`DiffPanel`, `@pierre/diffs`,
  parsed and syntax-highlighted in a worker pool, split/stacked modes), not
  inline in the chat.

**Zed's agent panel** does render inline: `ToolCard` for the edit-file tool
shows an embedded diff editor in the thread (zed-industries/zed#29234), plus a
persistent accordion above the composer summarising files and lines edited, and
per-hunk keep/reject.

**Cursor** applies edits live and pushes review into the editor's diff view
rather than the chat.

The convergent lesson: **turn-level change summary in the stream, full diff in a
dedicated surface.** Inline per-edit diffs (Zed) are additive, not the primary
affordance.

## Proposed Decision

Introduce the **turn** as a first-class row in the chat, and attach git-truth
change data to it. Four phases, each independently shippable.

### Phase 1 — Turn segmentation in `workLogPresentation`

Extend `PresentedChatRow` with a turn boundary and thread `turn_id` through
row derivation. `derivePresentedChatRows` gains a second pass that partitions
rows into turns keyed by `AgentChatEvent.turn_id`, falling back to
"user message starts a turn" when a provider omits `turn_id` (Codex and Gemini
transcripts do not always carry one — see `chat_transcript.rs`).

```ts
export type PresentedChatRow =
  | { kind: "event"; event: AgentChatEvent; entry?: PresentedWorkEntry }
  | { kind: "work_group"; id: string; entries: PresentedWorkEntry[]; changedPaths: string[] }
  | { kind: "turn_change_summary"; id: string; turn_id: string | null; turn_index: number | null };
```

A `turn_change_summary` row is emitted after the last event of a turn that
contains at least one write-evidenced work entry. Purely conversational turns
emit nothing. This phase is pure logic with no backend dependency and is
unit-testable in `workLogPresentation.test.ts`.

### Phase 2 — `TurnChangeCard` fed by change review

Add a turn-scoped variant of the existing change-review query rather than a new
subsystem. `ChangeReviewFileEntry.turn_indices` already carries the attribution;
the command needs a request field to filter to a single turn index, or the
frontend filters client-side from a conversation-scoped summary it already has
reason to hold.

Recommended: **a `turn_index` filter on the existing `load_change_review`
request**, keeping one attribution engine and one cache. A per-agent
`useTurnChanges` hook subscribes once per conversation and indexes entries by
turn, so N turn cards cost one invoke, not N.

`TurnChangeCard` mirrors the proven T3 Code shape, in Wardian's idiom:

- Collapsed header: `N changed files` + a `DiffStatLabel` (`+X` in
  `var(--color-wardian-success)`, `-Y` in `var(--color-wardian-error)`),
  `aria-label="X additions, Y deletions"`.
- Expanded: path-compacted directory tree, each file row carrying the
  `CHANGE_KIND_PRESENTATION` dot already defined in `ChangesPanel.tsx:46` —
  reuse it by lifting it to a shared module rather than duplicating.
- Row click reuses `ChangesPanel`'s existing navigation verbatim:
  `workbenchNavigation.open({ surface_type: "files", resource_key, state:
  changeSurfaceState(baselineForFile(...)) })`. `baselineForFile` and
  `changeSurfaceState` (lines 126 and 143) lift to a shared
  `features/changes/changeNavigation.ts` consumed by both surfaces.
- `evidence: "inferred"` entries render with a muted marker and a tooltip
  distinguishing them from `"attributed"`, preserving the honesty the Changes
  panel already maintains via `changeEvidenceLabel`.

Binary and truncated entries render as labels, never as a diff invitation —
`ChangesPanel` already sets this precedent.

### Phase 3 — Structured edit rendering for `tool_input`

Replace the text-sniffing diff path with structured rendering when the provider
supplied structured input. Add to `activityBlocks.ts` a resolver that reads
`metadata.tool_input.{old_string, new_string, file_path}` and synthesises a
minimal hunk, so a Claude `Edit` renders as an actual before/after instead of a
path chip. Keep `looksLikeDiff` as the fallback for providers that emit patch
text (`apply_patch`, Codex).

Render it collapsed by default with a `+X -Y` header, matching
`shouldCollapseActivity`'s existing discipline. This is the Zed `ToolCard`
affordance, scoped to data Wardian already has.

### Phase 4 — Work-log compression

Reduce `WORK_GROUP_MIN_ENTRIES` from 4 and the `slice(-6)` preview in
`WorkGroupRow` (`AgentChatView.tsx:480`). T3 Code's `MAX_VISIBLE_WORK_LOG_ENTRIES
= 1` is aggressive but correct in direction: the operator wants the current
action and a count, not a scrolling ledger. Propose latest-2 with an explicit
`hiddenCount` toggle, tuned against real transcripts rather than chosen up front.

Additionally, suppress `Read`/`Glob`/`Grep`-class entries from the visible
preview entirely when a turn produced writes — the change card now answers the
question those rows were being scanned for.

### Phase 5 — Adjacent affordances unlocked by turn segmentation

Catalogued from a full read of T3 Code's `apps/web/src/components/chat/`. These
are not required for Phases 1–4 but become cheap once turns exist as rows, and
several address defects in the current view.

**Turn folds.** `deriveTurnFolds` (`MessagesTimeline.logic.ts`) collapses a
*settled* turn's commentary and tool rows behind a `Worked for 2m 14s` row,
leaving the terminal assistant message visible below the fold. Guards worth
copying verbatim: never fold a turn with a streaming message, never fold the
unsettled turn, and never fold spawn/CTA rows whose work outlives the turn.
Wardian's workflow and delegation rows have exactly that outlives-the-turn
property.

**Scroll anchoring as an explicit state machine.**
`timelineScrollAnchoring.ts` names three modes — `following-end`,
`anchoring-new-turn`, `free-scrolling`. A new turn anchors its user message to
the top of the viewport rather than pinning to the bottom. Re-arming
live-follow uses a strict 40px band (`TIMELINE_FOLLOW_REARM_THRESHOLD_PX`)
because a near-end heuristic yanked readers back down mid-history. Disclosure
buttons carry `data-scroll-anchor-ignore` so expanding a row is not read as
scroll intent. `AgentChatView` currently has no anchoring model at all beyond
`hiddenOlderRowCount` and a latest-row key.

**Change-card auto-expand heuristic.** `shouldAutoExpandChangedFiles` expands
only on the latest turn, at ≤5 files and ≤200 total changed lines. Small
changes show themselves; large ones stay a header. Fold this into
`TurnChangeCard` in Phase 2 rather than defaulting to collapsed.

**Scope-aware collapsed preview.** `selectChangedFilePreview` picks up to three
files, **one per distinct top-level scope**, so the preview conveys breadth
instead of three files from the same folder.
`summarizeChangedFileScopes` renders `src 4 · docs 2` chips ranked by file
count. Both are better than Wardian's current `paths.slice(0, 6)`.

**`DiffStatLabel` layout modes.** An `aligned` variant uses
`grid-cols-[4ch_4ch]` so `+`/`-` columns line up vertically down a file list;
`inline` is used in headers. Worth building as a shared primitive with the
`aria-label="X additions, Y deletions"` contract from the start.

**Status-slot reservation and settle-aware tone.** Work rows reserve
fixed `size-4` slots for expand and status glyphs so a row does not reflow when
a tool resolves. Tone resolution is keyed on turn settledness: an unresolved
tool reads *neutral* while the turn runs and *success* once the turn settles.
Wardian's `activityTone` (`activityBlocks.ts:146`) marks success immediately
and has no neutral-pending state, so a mid-flight tool briefly claims an
outcome it does not have.

**Heading/preview de-duplication.** `PlainWorkEntryRow` drops the preview line
when it normalizes to the same string as the heading, and
`normalizeCompactToolLabel` strips trailing `complete`/`completed`. Wardian has
the inverse defect: for a `tool_call` with a generic title and a command,
`activityTitle` (`activityBlocks.ts:110`) falls back to the command *and*
`workEntrySummary` (`workLogPresentation.ts:235`) returns the same command, so
`WorkEntry` prints it twice — once bold, once mono. Fix alongside Phase 4.

**Stale approval detection.** T3 Code marks approvals that can no longer be
resolved because provider state was lost across a restart, rather than leaving
dead buttons. Wardian needs this *more* than T3 Code does: agent sessions here
routinely outlive the app, and `runtime_generation` already exists to detect
exactly that discontinuity. Import the detection.

**Clickable file references in prose.** `ChatMarkdown` resolves file paths and
`file.ts:10` forms inside inline code and links into navigation targets that
open the right panel and reveal the line. Wardian already has `ChatMarkdown` and
already has workbench file navigation; wiring them together is small and
independent of every other phase.

### Rejected: T3 Code decisions that do not transfer

Recorded so they are not re-proposed. Each is a sound choice *for T3 Code* and a
mismatch for Wardian.

**Composer-hosted approvals.** T3 Code moves approvals and user-input requests
out of the stream and into the composer. Wardian must keep them inline.
Approval here is already a **fleet-level** signal — `commands/inbox.rs`
(`list_inbox_notifications`, `resolve_inbox_notification`), the Queue surface,
the Watchlist, and the documented Amber "Action Required" status all answer
"which agent needs me". The chat's job is the complementary one: showing *which
tool call* provoked the request. Moving it to the composer would duplicate the
Queue and detach the request from its cause. Take the stale-approval detection
above; leave the placement alone.

**Timeline minimap.** Designed against a single 768px centered column with wide
side gutters — `resolveTimelineMinimapHasPersistentGutter` requires ≥48px of
gutter before the rail even renders. Wardian's chat is hosted by
`AgentsOverviewView` inside an auto-computed multi-column agent grid
(`agentsOverviewLayout.ts`), where that gutter does not exist. Cross-session
navigation is already the Roster and Watchlist's job.

**Per-message revert.** T3 Code gives each thread its own git worktree, which is
what makes "revert to this message" safe there. Wardian points multiple agents
at shared workspaces through junctions — `ChangeReviewFileEntry.agent_ids` is a
*list* per file precisely because concurrent authorship is expected. Reverting a
turn in a shared workspace can silently discard another agent's work. The
capability may still be worth building on `change_snapshot.rs`, but it needs its
own spec with a concurrent-authorship safety model, and it is not a chat-view
change.

**Prompt stash.** Twenty prompts with attachments in `localStorage`. This
conflicts with State Sovereignty and Markdown-as-Truth: composer content that
survives a restart is state, and Wardian's state lives on disk under
`~/.wardian/` with the Rust backend as its authority. If the need is real, it is
a backend feature, not a zustand store.

**Context window meter.** Presupposes uniform token accounting. Wardian
normalizes heterogeneous CLI transcripts, and several providers report no usage
at all — some are PTY-scraped. A meter that is blank or wrong for half the
roster is worse than no meter. Revisit only if per-provider usage lands in
`AgentChatEvent` metadata.

**Banner stack severity cap.** A well-observed micro-detail that presupposes
several simultaneous composer banners. Wardian does not have that density.

**`MAX_VISIBLE_WORK_LOG_ENTRIES = 1`.** T3 Code can show a single work row
because turn folds, the changed-files card, and a plan sidebar already carry the
information. Phase 4 must not adopt the number before Phases 1–2 ship, and the
final value must be tuned against real Wardian transcripts rather than inherited.

**Auto-expand thresholds (5 files / 200 lines).** The *principle* — expand small
changes, collapse large ones — transfers. The constants are T3 Code's tuning
against their own corpus and carry no evidence for ours.

### Explicitly out of scope

- Replacing `CodePanel` with a worker-pool diff renderer. Wardian's diffs are
  turn-sized, and the Files workbench surface already owns full comparison.
- Per-hunk accept/reject. That is a checkpoint/revert capability requiring
  backend work well beyond a chat-view change, and Wardian's snapshot model
  (`change_snapshot.rs`) should drive its design, not the chat.
- Inline line commenting as composer context (T3 Code #79, Codex). Worth a
  separate spec; it depends on Phase 2 landing first.

## Consequences

- **Positive**: The chat answers "what changed" from git truth rather than from
  path-string heuristics, with change kind and line counts.
- **Positive**: No new attribution engine. Phase 2 reuses `load_change_review`,
  its evidence model, and its workbench navigation, so the Changes panel and the
  chat cannot disagree about what an agent touched.
- **Positive**: A turn becomes addressable in the UI, which is the prerequisite
  for per-turn revert, per-turn diff links, and turn folding later.
- **Positive**: Phase 3 recovers edit content already being discarded, with no
  provider or backend change.
- **Negative**: Change data becomes git-dependent inside the chat. Non-git
  workspaces get the turn card degraded to claimed paths only — the same
  degradation `ChangesPanel` already announces via `git_available`.
- **Negative**: Turn segmentation depends on `turn_id`, which providers populate
  inconsistently. The user-message fallback is a heuristic and will mis-segment
  some transcripts; this needs per-provider fixture tests.
- **Negative**: A per-conversation change-review subscription adds recompute
  pressure on every turn boundary. Mitigated by the existing generation-guard
  pattern in `ChangesPanel.recompute`, but it must not become one invoke per
  visible card.
- **Negative**: Phase 4 hides information some operators currently rely on.
  It must be reversible via the expand toggle, never a silent drop.

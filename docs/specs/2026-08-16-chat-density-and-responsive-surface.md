# Chat Density and Responsive Surface

- **Status:** Implemented (initial slice)
- **Date:** 2026-08-16

## Context

Wardian's chat view already normalizes conversation messages, tool activity,
approvals, work groups, terminal fallback, and change summaries. The current
presentation gives too many of those records full-width container treatment.
That makes a compact Grid card feel crowded, makes the conversation harder to
follow, and causes the same layout to feel oversized when maximized.

Beautiful UI's useful reference is its progressive-disclosure model: thinking,
streaming, tool chips, task rows, approvals, and diffs are distinct primitives
with different visual weights. Wardian should adopt that hierarchy while
preserving its own tactile, transparent, high-tech operating-surface identity.

## Design direction

The chat surface is a **compressed mission timeline**:

- Conversation prose is the primary content and receives the most readable
  width and spacing.
- Agent work is a quiet status rail by default: compact, scannable, and
  expandable when the operator needs evidence.
- Decisions and failures interrupt the rail with deliberate, high-salience
  surfaces.
- The surface should feel inspectable without making every internal event
  compete for attention.

## Goals

- Reduce default transcript height without hiding important conversation.
- Hide routine tool calls and outputs behind compact work-log rows by default.
- Keep approvals, errors, and meaningful diffs immediately discoverable.
- Make the same transcript adapt to Grid cards, maximized desktop chat, and
  narrow/mobile remote views.
- Preserve keyboard access, copy actions, file opening, approval responses, and
  terminal fallback behavior.
- Use existing Wardian theme variables and semantic status colors.

## Non-goals

- Replacing the normalized `AgentChatEvent` model or provider adapters.
- Removing the raw terminal fallback.
- Adding a new command palette, source picker, or model workflow to the
  composer in this slice.
- Rebuilding the whole Grid shell or changing navigation.

## Visual hierarchy

### 1. Conversation rows

User prompts remain compact, right-aligned, and visually distinct. Assistant
prose becomes the calmest and most readable surface: left-aligned, full-width
within the transcript column, and not enclosed in a heavy card by default.

Assistant markdown keeps code, links, tables, and copy actions. Paragraphs use
short vertical rhythm; long responses are allowed to breathe in maximized mode
but remain bounded by the card width in Grid mode.

### 2. Work rail

Routine tool calls, successful tool results, status transitions, and terminal
fallback previews are represented as compact rows with:

- a semantic status dot;
- a short verb/title such as `Edit`, `Run`, `Search`, or `Read`;
- one-line detail such as a path, command, or result summary;
- a disclosure control only when additional content exists.

Routine rows are collapsed by default. A contiguous run of routine activity is
grouped into one `Work log` row with a count and the latest useful summary.
Expansion reveals the existing detailed activity rows without changing the
underlying event order.

### 3. Intervention surfaces

Approval-required activity, failed activity, and actionable errors remain
expanded enough to support a decision. They use amber or red semantic tokens,
not decorative emphasis. Approval choices stay keyboard reachable and the
composer continues to support typed responses.

### 4. Checkpoints

Turn change summaries remain after the turn they describe. They are compact
checkpoints showing changed-file count, line totals, and paths; they should not
read as another assistant message or another full activity card.

## Responsive behavior

The transcript should use container-driven behavior because Grid card width is
more important than viewport width.

### Compact card: under 420px content width

- Hide secondary activity metadata such as provider source and timestamps.
- Use one-line work rows with ellipsis and preserve the disclosure button.
- Keep user prompts and approval choices full-width enough for touch.
- Limit visible changed-file chips and expose the remainder as a count.
- Keep composer controls to attachment, model selection, and send/interrupt.

### Standard card: 420px to 720px

- Show title plus one detail line for work rows.
- Keep routine work grouped and collapsed.
- Allow assistant content to use the full inner width.
- Display code and terminal content in bounded scroll regions rather than
  expanding the card indefinitely.

### Maximized: above 720px

- Allow longer assistant prose measure and richer activity metadata.
- Keep work groups collapsed initially; do not turn extra width into extra
  default output.
- Let expanded code/diff/terminal content use a wider bounded panel.
- Preserve a stable composer height and a clear latest-response anchor.

### Narrow/mobile remote view

- Stack all metadata and controls rather than relying on hover.
- Keep interactive targets at least 44px where touch input is expected.
- Use full-width approval options and a bottom-pinned composer.
- Never hide the core prompt, response, approval, or interrupt actions.

## State behavior

The first implementation must make these states visually distinct:

1. Empty/loading: teach that the transcript is waiting for provider events.
2. Thinking: one quiet live row, not a full card.
3. Streaming: assistant content updates in place without duplicate rows or
   scroll jumps.
4. Routine work: grouped, collapsed work rail.
5. Approval required: expanded amber intervention surface.
6. Failure: expanded red intervention surface with copyable evidence.
7. Completed turn: assistant response followed by a compact change checkpoint.

## Implementation shape

Keep the existing transcript data flow and refine the presentation layer:

- `AgentChatView` owns the responsive transcript container and scroll policy.
- `ChatTranscriptRows` owns row hierarchy and progressive disclosure.
- Existing activity-block derivation remains the source of labels, tones,
  changed paths, structured edits, and copy payloads.
- Add container-query styles or equivalent width-aware classes rather than
  viewport-only assumptions.
- Avoid adding a new visual card wrapper around every row.

## Validation

- Add rendering tests for collapsed routine work, expanded work, approval,
  failure, and compact-width metadata behavior.
- Preserve existing copy, open-file, approval, composer, and scroll tests.
- Capture feature-specific screenshots for compact Grid card, maximized chat,
  and approval-required states under
  `e2e/screenshots/chat-density/<timestamp>/`.
- Run frontend lint, unit tests, and build. Run native validation only if the
  implementation changes PTY or native IPC behavior.

## Rollout slices

1. Introduce the visual hierarchy and default-collapsed routine work rail.
2. Add container-driven compact/standard/maximized treatment.
3. Tune approval, failure, diff, and composer states with screenshot evidence.
4. Revisit richer prompt-bar affordances only after the transcript reads well
   at all target widths.

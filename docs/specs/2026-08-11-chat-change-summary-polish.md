# Chat Change Summary Polish

- **Status:** Implemented
- **Date:** 2026-08-11

## Decision

Keep one turn-level change card in the transcript and place it immediately
after the final rendered row of the turn it summarizes. The card presents only
the operator-relevant facts: changed-file count, reported line totals, and
file paths.

Remove explanatory provenance and scope wording from the visible card. Those
details describe how the data was collected rather than helping the operator
understand the conversation. Unknown per-file counts remain visible as an
em dash, and the accessible statistics label retains partial-count coverage.

Turn segmentation continues to use user messages as boundaries. The summary
is emitted when that boundary is encountered, so a later user message cannot
absorb the previous turn's changes. The pure transcript test includes the
assistant response and next user prompt to pin this ordering.

## Consequences

- Chat history reads as a conversation with a compact change checkpoint after
  the work it describes.
- The card does not expose implementation provenance or internal transcript
  scope as conversational copy.
- Provider evidence rules and the existing remote partial-history behavior are
  unchanged.

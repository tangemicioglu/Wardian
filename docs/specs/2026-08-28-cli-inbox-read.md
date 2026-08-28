# CLI Inbox Read Path

## Status

Accepted for the Inbox CLI slice requested by issue #1006.

## Decision

Add `wardian inbox list` as a read-only CLI surface over the Inbox projection.
When the desktop app is running for the same `WARDIAN_HOME`, the CLI asks the
app for the assembled projection used by the remote Inbox. If the app is not
running, the CLI falls back to persisted queue items, durable interaction
records for `notify update` and `notify approval` events, and workflow-run
checkpoints for awaiting approvals and terminal outcomes.

The command returns schema-versioned JSON, sorted newest first, with bounded
pages. It supports:

- `--type <type,...>` for `action_needed`, `agent_update`, `agent_completed`,
  `workflow_completed`, `workflow_failed`, and `approval_request`.
- `--source <source,...>` for evidence sources such as `provider_runtime`,
  `interaction_store`, and `live_runtime`.
- `--unread`, `--limit <n>`, and `--offset <n>` for polling and pagination.

Reading has no side effects. The command does not mark, dismiss, or resolve
Inbox items. Existing write commands remain the authoritative paths for agent
visibility: `wardian notify update` communicates a material result or
limitation, while `wardian notify approval` requests a decision for a
consequential action. Routine progress remains in the transcript.

## Rejected Scope

This slice does not add `show`, `watch`, `mark-read`, or a new agent-specific
Inbox store. Those can be layered on the stable item IDs and projection schema
without changing the read/write boundary established here.

## Verification

The CLI parser, control request/response schema, persisted fallback, filtering,
and paging behavior are covered by focused Rust tests. Full repository
validation remains the release gate for changes that are committed or opened
as a pull request.

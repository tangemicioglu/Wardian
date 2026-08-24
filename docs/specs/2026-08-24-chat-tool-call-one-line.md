# Chat tool-call one-line presentation

## Context

The normalized chat transcript already carries the actual invocation in
`AgentChatEvent.command`. Some provider adapters also carry lifecycle labels
such as `exec_command_begin`, `exec start`, or `exec running` in `title`. The
shared chat renderer currently gives those labels visual priority. A single
command-only event can therefore look like provider plumbing, while a grouped
work log makes the command secondary or requires expansion to read it.

This affects both desktop grid chat and the remote mobile chat because they
consume the same `ChatTranscriptRow` renderer.

## Decision

- Treat provider lifecycle labels as low-signal titles when an actual command
  is present.
- Make the command the primary visible text for those events and for collapsed
  grouped summaries.
- Render a command-only single tool call whose title is only provider plumbing
  as one compact row by default. The row keeps the exact command and the
  existing copy action, while named tool titles, successful output, failures,
  approvals, diffs, structured edits, and changed-file evidence keep their
  existing detailed surfaces.
- Do not change the backend DTO or discard transcript evidence.

## State walk

For `{ kind: tool_call, title: "exec_command_begin", command: "npm test" }`,
the presentation layer now resolves the title to the command, recognizes that
the event has no separate output, and renders `$ npm test` as the default row.
For a larger adjacent batch, the same presented entry is used by the collapsed
work-log summary, so the latest command is visible without opening the group.

## Verification

Focused tests cover lifecycle-labelled command titles, the compact single-call
row, grouped summaries, and the shared transcript row used by desktop and
remote mobile chat.

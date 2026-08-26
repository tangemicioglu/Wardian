# Chat mobile lifecycle visibility

## Status

Accepted for issue #982 as a follow-up to issue #954.

## Problem

The shared desktop and remote-mobile chat renderer handled `exec start` and
`exec_command_begin`, but not present-participle labels such as `exec starting`.
Command-less lifecycle events could also survive filtering because their label
was mistaken for a meaningful tool identity.

## Decision

- Classify present-participle lifecycle labels as low-signal provider metadata.
- Keep a command-bearing lifecycle event as a compact command row.
- Hide command-less in-flight lifecycle events from both desktop and remote
  mobile chat.
- Verify the behavior through the actual `RemoteMobileApp` route.

## Invariants

1. A concrete command remains visible when a provider lifecycle label is present.
2. `exec starting` and `exec running` do not appear as standalone labels.
3. Named tools and actionable approvals remain visible.
4. Desktop and remote-mobile chat continue to use the same presentation rules.

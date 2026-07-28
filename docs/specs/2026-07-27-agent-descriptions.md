# Agent Descriptions

**Date:** 2026-07-27
**Status:** Implemented

## Problem

Agent names and classes stop being enough when a roster contains many similar
agents. A person or another agent needs a quick reminder of what one specific
agent is responsible for before selecting or delegating to it.

Issue #728 proposed a broader runtime capability and permission profile. That
proposal does not match the immediate user need: an operator-authored memo is
more honest than inferring capabilities from provider configuration, and it
does not require Wardian to claim that configured tools are currently usable.

## Decision

Each persisted `AgentConfig` has a `description` string containing an optional,
human-authored purpose memo of at most 280 characters.

- New agents default to an empty description. The spawn form has no new field,
  so creating an agent requires no extra work.
- The Agent Config pane can add, edit, or clear the description later.
- Changing only the description does not restart the provider.
- The roster keeps its existing two-line density and renders
  `<class> · <description>` on the metadata line when a description exists.
- Agent and workflow-assignment searches include descriptions. The workflow
  assignment picker also shows the memo below the existing runtime metadata.
- `wardian agent` includes `description` in its default identity output, and
  `wardian agent update <target> --description <memo>` updates or clears it.
- SQLite mirrors the description so CLI identity reads remain available when
  the desktop runtime is not the status source.

Descriptions are trimmed at the backend boundary. Empty text clears the memo.
The 280-character bound applies to Unicode scalar values rather than UTF-8
bytes.

## Boundaries

- A description is metadata. It is not injected into provider prompts or class
  instructions.
- It does not declare, grant, inspect, or verify skills, tools, permissions, or
  runtime availability.
- Class descriptions remain reusable class metadata; agent descriptions explain
  one particular agent instance.
- Clones retain the source description because they begin as copies of that
  agent's configuration. Users can edit the memo independently afterward.

## Verification

- Core model and database tests cover backward-compatible defaults, serialized
  round trips, migration, and offline identity reads.
- Backend tests cover trimming, clearing, length rejection, live-state updates,
  and no-restart update semantics.
- CLI tests cover parsing and default JSON output.
- Frontend tests cover editing without a restart prompt, roster rendering, and
  description-aware search and workflow assignment.

# Agent List Density

**Date:** 2026-08-30
**Status:** Implemented

## Problem

The compact agent roster has limited horizontal and vertical space. Rendering an
agent's purpose memo beside its class makes rows wrap or truncate before the
operator can scan the roster efficiently.

## Decision

- The global desktop and remote watchlist heading is **All Agents**.
- Roster rows show the agent name and class only; they do not render the agent's
  optional purpose description.
- Agent descriptions remain persisted and editable in Agent Configuration.
  Description-aware roster search and automation-assignment search remain
  available even though the compact row does not display the memo.
- Custom watchlist headings continue to use their saved names.

This supersedes the roster-rendering detail in
[Agent Descriptions](./2026-07-27-agent-descriptions.md) without changing the
description data model or its configuration and search behavior.

## Boundaries

This is a presentation-only change. It does not alter agent instructions,
provider sessions, persisted descriptions, filtering, sorting, or automation
assignment behavior.

## Verification

The desktop and remote watchlist tests plus the agent lifecycle browser test
verify that the global heading reads **All Agents**, descriptions are absent
from compact roster rows, and descriptions remain visible in Agent
Configuration.

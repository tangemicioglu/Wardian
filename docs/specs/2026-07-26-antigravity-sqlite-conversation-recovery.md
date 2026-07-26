# Antigravity SQLite Conversation Recovery

## Status

Implemented.

## Problem

Antigravity 1.1.7 records interactive conversation steps in
`conversations/<conversation-id>.db`. Its legacy brain transcript can remain
empty even while the database contains the user prompt and model responses.
Wardian watched only that legacy JSONL path, so it neither displayed the real
conversation nor restored its provider identity after a restart when the saved
identity was missing.

## Decision

Wardian resolves an Antigravity conversation only from the provider's explicit
workspace cache entry. When available, `conversation_metadata.json` must also
bind that conversation to the same workspace URI. The resolver never selects a
conversation by file recency.

For a verified mapping, Wardian prefers the SQLite conversation database when
it contains message steps, falls back to the legacy JSONL transcript for older
providers, and turns database user and completed planner-response steps into
Chat messages. A normal restored agent backfills its missing `resume_session`
from this verified mapping before spawning, so the provider receives the exact
`--conversation` argument.

**Clear** records the prior Antigravity conversation in a persisted exclusion
list. That prevents cache recovery from reconnecting a deliberately cleared
session; only a changed workspace mapping from the new session may become its
identity.

## Consequences

- Chat renders the durable provider conversation rather than an empty legacy
  transcript.
- Restarts recover the exact workspace-bound provider session without a
  cross-workspace or newest-file guess.
- A Clear action remains a hard boundary between conversations.

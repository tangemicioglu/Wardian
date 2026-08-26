# Last Queried Telemetry

## Decision

The watchlist's Last Queried column represents the most recent user prompt
timestamp for each agent. It is independent of the current Wardian process
or app launch time.

New watchlist preferences show only Last Queried by default. Status, Query
Count, Uptime, and Provider / Model remain available through the column picker.
Existing saved preferences are preserved.

## Data sources

The backend exposes `last_query_timestamp` with agent telemetry. It combines
these sources and keeps the newest valid timestamp for each agent:

1. User-authored `Message` interaction records with no sender agent, which
   cover prompts delivered by Wardian UI, CLI, remote, and headless paths.
2. Provider transcripts, including real user-message records for Codex,
   Claude, Gemini, Pi, and both Antigravity's current SQLite step metadata and
   legacy JSONL format, plus prompt-loop timestamps for OpenCode.
3. The existing watchlist interaction map remains a frontend compatibility
   fallback for records created by older versions or direct terminal input
   that has not yet appeared in a provider transcript.

Agent-authored interactions are excluded. A telemetry refresh may migrate a
provider-derived timestamp into the legacy map, but it must never replace a
newer timestamp with the current app time.

## Restart and parsing behavior

Telemetry reads only senderless message records from the persisted interaction
ledger on each metrics pass. Provider logs are bounded to the existing
telemetry tail limit and are reparsed when their modification time changes.
Current Antigravity conversations are read from the SQLite `steps.metadata`
protobuf timestamp; legacy Antigravity JSONL remains supported. An initial
transcript replay supplies the historical timestamp without changing the
agent's current status.

The UI uses the telemetry timestamp first and falls back to the legacy map
only when telemetry has no timestamp. Relative display formatting remains a
presentation concern; persisted values are ISO 8601 timestamps.

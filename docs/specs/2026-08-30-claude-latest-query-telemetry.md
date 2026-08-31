# Claude Latest-Query Telemetry

## Decision

Claude's `Last Queried` telemetry is derived from provider transcript records
whose message content represents a real user prompt. It must not depend on the
agent's current status: Claude can be idle or processing while the latest
prompt timestamp remains the historical user-message time.

## Provider boundary

Claude transcript `parentUuid` is lineage metadata present on ordinary user
messages as well as provider-injected context. It is retained as a causal
reference, but is not by itself context evidence. Context classification
requires an explicit provider marker such as `isMeta`/`isContext` or a parent
tool-use reference. Provider interruption records and local command records do
not count as user prompts.

The timestamp is read from the transcript record's top-level `timestamp` and
merged with the durable per-agent watermark using the newest valid value.

## Regression coverage

The Claude provider tests cover a normal user prompt with `parentUuid`, an
interruption record, and explicit native context markers. Telemetry tests
verify that a normal parent-linked prompt is accepted and advances the latest
query timestamp.

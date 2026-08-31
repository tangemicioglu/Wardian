# Claude Latest-Query Telemetry

## Decision

Claude's `Last Queried` telemetry is derived from provider transcript records
whose message content represents a real user prompt. It must not depend on the
agent's current status: Claude can be idle or processing while the latest
prompt timestamp remains the historical user-message time.

## Provider boundary

Claude transcript `parentUuid` is lineage metadata present on ordinary user
messages as well as provider-injected context. It remains in the raw
transcript; the adapter copies it into normalized `causal_ref` only for an
explicitly classified context record that has no parent tool-use reference.
It is not by itself context evidence. Context classification requires an
explicit provider marker such as `isMeta`/`isContext` or a parent tool-use
reference. Provider interruption records are normalized as
`provider_internal` events so they remain in the lossless transcript and
conversation archive; neither interruptions nor local command records count as
user prompts.

The timestamp is read from the transcript record's top-level `timestamp` and
merged with the durable per-agent watermark using the newest valid value.

## Regression coverage

The shared Claude classifier tests cover a normal user prompt with
`parentUuid`, both interruption records, and explicit native context markers.
Normalized-chat tests verify that interruption records remain as
`provider_internal` messages. The latest-query telemetry path consumes the
same `RealQuery` classification, so these tests cover its acceptance and
exclusion boundary without dropping provider-internal archive evidence.

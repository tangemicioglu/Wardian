# Conversation Input Provenance

## Decision

Wardian classifies message-shaped provider records by normalized input origin,
not by the provider display role. The archive recognizes four origins:

- `human_input`: a human request observed at the provider boundary.
- `agent_input`: a message delivered by another Wardian agent or by Wardian's
  generated delivery path.
- `context_injection`: provider-supplied context, including skill bodies, host
  context, and memory hydration serialized as a user-role message.
- `provider_internal`: provider bookkeeping or internal message content.

The classification is preserved in normalized provider-event metadata and in
`conversation.jsonl` records as `input_origin`, `input_purpose`, and, when the
provider supplies one, `causal_ref`. Context records may additionally carry
`request_root_id`. `request_root_id` identifies the stable human or Wardian
request that owns an injection. `causal_ref` points to the provider event or
request that caused the record.

Only `human_input` and `agent_input` message records start a request-indexed
turn. Context and provider-internal records remain in the lossless archive and
are attached to the current or causally identified root turn. An injection
without a recoverable root is indexed as `context_injection` with
`context_only` status, never as a pending request.

## Provider boundary

Provider adapters own the evidence used for classification. Claude explicit
metadata such as `isMeta`/`isContext` and `parent_tool_use_id` identifies
context records without inspecting their text. `parentUuid` is retained as
transcript lineage and normalized `causal_ref`, but is not sufficient by itself
to classify a record as context. Claude `Skill` tool-use IDs provide the causal
link for multiple skill injections in one request.

Providers that do not expose an equivalent context event retain their normal
user-role input classification and do not fabricate a context boundary. Their
limitation is reported as `context_observation: "unreported"` on the
normalized event, with the absence of `context_injection` evidence; the raw
provider record remains available for later adapter improvements.

OpenCode is an exception to that fallback: its provider-owned SQLite parts mark
synthetic editor context with `metadata.kind: "editor_context"` while the
parent message remains `role: "user"`. Those parts are classified as
`context_injection`, linked to the most recent real request, and retain a
provider-message causal reference. Codex also exposes a structural boundary:
provider-supplied host context is emitted as a `response_item` user message
with batched `content`, while the canonical human request is emitted as the
separate `event_msg` `user_message` record. Codex response-item context is
classified as `context_injection`, carries its provider message and
passthrough-turn references, and is attached to the canonical request root
without inspecting or matching the injected text.

## Compatibility and regeneration

The provenance fields are optional for backward compatibility with existing
archives. Turn derivation first uses fields persisted on the narrative record,
then recovers classification and root references from the archived normalized
provider events. This makes `turns.jsonl` regeneration deterministic whenever
the provider supplied sufficient evidence, without rewriting raw provider
ordering or event/source references. When a rooted context record precedes its
request record, derivation holds that context until the root request is seen;
an unmatched rooted context remains a context-only turn.

The existing `turns.jsonl` schema remains version 3. Readers must use the
request kind and status rather than the physical manifest row count when
counting human or agent tasks; `context_injection` rows are intentionally
retained for auditability.

## Conformance fixtures

Archive and transcript tests cover Claude skill calls followed by one or more
user-role injections, Claude context without a dedicated skill call, OpenCode
synthetic editor-context parts, Codex response-item host context, providers
without observable context evidence, legacy records regenerated from archived
events, and normal human and Wardian-agent inputs.

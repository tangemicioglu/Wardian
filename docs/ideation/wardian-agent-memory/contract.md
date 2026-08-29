# Wardian Agent Memory — Mission Brief

Status: Approved
Date: 2026-08-23
Supersedes: `docs/specs/2026-04-17-evidence-first-memory.md`

## Mission

Give every Wardian agent a provider-neutral, local-first memory that reliably appears at the start of a provider process. Memory is not a conversation archive and provider session history is not memory. SQLite is the authority; generated instructions and chat rows are observable projections.

## Product contract

- Direct retention and startup recall are enabled by default and require no curator model call.
- Memory belongs to one agent. A record is either agent-wide or bound to a workspace.
- A saved revision contains normalized recall text, a durable evidence excerpt, an integrity hash, and optional source links. Conversation and memory retention remain independent.
- Stable memories persist until superseded or removed. Current-state memories remain available, carry verification timestamps, and can be labeled stale; Wardian never silently expires them.
- Fresh provider processes receive bounded `Stable memory` and `Current state` sections. Resumed processes receive the relevant delta from their last injected fingerprint.
- Successful writes and startup loads are visible as compact, expandable memory events. The exact injected context is inspectable.
- Automated consolidation is off by default. If enabled, it is an ordinary bundled Wardian automation using the provider, model, and effort selected by the user. It consumes that provider's quota and has no hidden fallback.
- Only an explicit, validated `memory_commit` automation node may mutate memory. A generic session-close invoker can run any automation; it is not a memory-specific trigger or control surface.

## Delivery

1. SQLite authority, lifecycle API, CLI, scopes, provenance, revisions, audit events, and direct-retention instructions.
2. Provider-neutral startup compilation, full/delta fingerprints, deterministic budgets, and observable chat events.
3. Bundled consolidation automation, generic session-close invoker, strict structured output, archive cursors, and idempotent commit.

## Acceptance

- Two agents can store similarly worded memories without leakage.
- Workspace state is recalled only in its matching workspace; agent-wide preferences follow the agent.
- A fresh process receives the full brief before work and a resumed process receives only changed revisions.
- Superseded and removed revisions do not appear in recall, but remain auditable.
- A retry with the same idempotency key does not create another revision.
- A worker can emit no relevant memories and startup remains a reliable, observable miss.
- Live GPT-5.6-Luna agents demonstrate save, fresh recall, resumed delta recall, and isolation against a temporary Wardian home.

## Deferred

Semantic embeddings and reranking, shared or cross-agent scopes, provider-native history as memory, cascading deletion, and memory-specific automation reset/fork/compare controls.

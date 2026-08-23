# Agent-owned memory

Status: Implemented
Tracking: [#727](https://github.com/wardian-app/Wardian/issues/727)

## Decision

Wardian provides provider-neutral long-term memory through a local SQLite
authority at `<WARDIAN_HOME>/memory.db`. Records are owned by one Wardian agent
and are either agent-wide or bound to a canonical workspace. Conversation
archives and provider-native histories remain independent evidence/runtime
systems; neither is the memory authority.

Direct retention and startup recall are the default. They do not call a model.
Automated consolidation is optional, disabled by default, and implemented as an
ordinary bundled workflow with ordinary provider/model/effort assignments.

## Record lifecycle

Each logical memory has immutable revisions. Updating an active revision marks
it superseded and inserts its successor in one transaction. Removing memory
creates a retained tombstone state. Active recall excludes superseded, removed,
and integrity-invalid revisions. Stable memory has no expiry. Current state is
labeled stale after 30 days without verification but is not deleted.

Every revision owns normalized recall text, a durable evidence excerpt and
SHA-256 hash, and optional source links. Source links can open a conversation,
artifact, file, or workflow run for deeper inspection. Deleting either system
does not cascade into the other.

## Startup recall

Before provider launch, Wardian selects active agent-wide memory plus active
memory for the resolved workspace. A deterministic 12,000-character policy
orders stable before current records and reports omissions. A fingerprint covers
scope, logical ID, revision, kind, evidence integrity, text, staleness, and the
budget-policy version.

Fresh processes receive full `Stable memory` and `Current state` sections.
Restored/resumed processes compare against the latest fingerprint for their
provider process key and receive additions, revisions, removals, and stale-state
changes. When nothing changed, Wardian re-sends the active checkpoint because a
previous process may have exited before its model consumed the compiled brief.
Empty recall creates no synthetic turn. Storage/compile/audit failures fail open
for provider startup and are logged; save/update operations fail closed.

The brief and direct-retention contract are appended only to Wardian-generated
habitat instructions. Providers receive the generated context through their
normal instruction projection; Codex also receives it through a runtime
developer-instructions override because its real workspace remains the process
working directory. No user-authored instruction file is changed.

## Workflow boundary

`memory_commit` is the only workflow node that can mutate memory. It consumes a
strict `MemoryCommitBatch`, validates the complete request, and commits memory
operations, archive cursor movement, audit events, and an idempotency receipt in
one transaction. Replaying the same key and request returns the original result;
reusing a key for different content is rejected.

The bundled `memory-consolidation` workflow is seeded like every other sample:
only when missing, never auto-run, and never overwritten after user edits. A
generic persisted session-close invoker can launch any workflow after a matching
conversation boundary. New invokers are disabled unless explicitly enabled.
Failures are workflow failures and do not roll back the agent lifecycle action.

Temporary-provider workflow assignments may optionally specify a model and
effort. This is normal workflow configuration, not memory-specific behavior.

## Observability

Memory save, update, remove, and non-empty load actions are stored independently
of provider logs and rendered as dedicated compact chat events. `Memory loaded`
expands to the exact injected context. Local and remote chat share the renderer.
Memory events are deliberately excluded from conversation archives.

## Deferred

Local embeddings and semantic reranking, shared/cross-agent scopes, cascading
deletion, and memory-specific workflow reset/fork/restore controls.

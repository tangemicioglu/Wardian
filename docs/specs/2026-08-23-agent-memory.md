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

At the end of every user task, the provider independently checks whether the
user established a clear durable preference, project convention, decision,
correction, lesson, or current state. It saves that context in the same turn
even when the request is brief and does not use words such as "remember" or
"save". This is a required pre-final-answer lifecycle step, and the runtime
instruction includes the basic save/list/update syntax so the provider need not
load a separate skill first. Ambiguous and explicitly transient instructions
are not retained.

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

Every successful non-empty provider delivery records its own injection receipt,
even when the compiled checkpoint is unchanged. Headless delivery becomes
successful only after a zero exit; interactive delivery becomes successful only
after provider-ready evidence. A failed or pre-readiness launch records no
receipt, so its replacement receives the required context again. A repeated
fingerprint means the memory content is unchanged; it does not mean a later
provider process consumed an earlier delivery.

Fresh background workflow workers keep their synthetic provider-process
identity, but recall and managed memory commands use the registered agent named
by the assignment. This keeps workflow process isolation without creating a
second memory owner.

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

The engine renders the target `agent_id` into `memory_commit` only from the
canonical `{{trigger.output.agent_id}}` invocation value; model-produced
structured output cannot select the memory owner. Trigger input is not an
authority boundary. The executor separately requires an immutable authenticated
invocation principal supplied by the trusted session-close context or by a
managed CLI process whose launch-scoped memory capability the desktop validates.
It rejects unauthenticated commits and any rendered request or batch whose agent
identity differs from that principal. The principal persists with the run so an
approval resume cannot lose or replace the original authority.

Session-close workflow launch is a post-commit lifecycle effect. Wardian starts
it only after the clear/rollover succeeds, and every boundary receives a unique
idempotency component even when no archive is available. Concurrent invoker
edits are serialized under a filesystem lock so independent CLI and desktop
writes cannot overwrite one another.

Inside a Wardian-managed provider process, `wardian memory` authorizes only the
agent identified by `WARDIAN_SESSION_ID` and a matching runtime-issued
`WARDIAN_MEMORY_CAPABILITY`. Wardian stores only the capability hash and permits
concurrent provider processes for the same agent, so changing the claimed
session ID is not sufficient to impersonate another agent. Each process owns a
separate lease owned by its `ActiveAgent`; Wardian revokes it when terminating,
replacing, or reclaiming that runtime, not merely when a PTY reader fails.
Another agent's name or UUID is rejected for list, show, save, update, history,
and remove. An operator shell
without a managed identity retains administrative access. Offline name lookup
uses persisted roster state and must resolve to a unique agent UUID.

## Observability

Memory save, update, remove, and non-empty load actions are stored independently
of provider logs and rendered as dedicated compact chat events. `Memory loaded`
expands to the exact injected context. Local and remote chat share the renderer.
Memory events are deliberately excluded from conversation archives.

Native GPT-5.6-Luna acceptance covers explicit save and isolation, fresh and
delta recall, implicit capture from ordinary tasks, correction/supersession,
agent-wide versus workspace scope, rejection of one-response-only formatting,
and later authoritative recall.

## Deferred

Local embeddings and semantic reranking, shared/cross-agent scopes, cascading
deletion, and memory-specific workflow reset/fork/restore controls.

# Agent memory architecture

The authority is `crates/wardian-core/src/memory.rs`. It opens the Wardian-home
`memory.db` with WAL, foreign keys, a busy timeout, and transactional lifecycle
operations. The CLI and Tauri commands call the same store.

Startup integration lives in `src-tauri/src/manager/spawn.rs`; generated
instruction projection lives in `src-tauri/src/utils/fs.rs`. Codex uses the same
generated text through a runtime `developer_instructions` config override so
Wardian does not modify the real workspace's `AGENTS.md`. Provider code must not
select or persist memory. Keep startup compilation deterministic and free of
model calls.

Keep the direct-retention instruction self-contained. It must include basic
save, list, and update command syntax and identify the retention check as a
required pre-final-answer step. Do not make reliable default retention depend on
the provider first locating or reading the Wardian CLI skill.

Headless execution has two identities when a fresh workflow worker represents a
registered agent: `wardian_session_id` remains the synthetic provider-process
key, while `memory_agent_id` is the registered owner used for recall, injection
auditing, and the child process's `WARDIAN_SESSION_ID`. Direct headless delivery
uses the same real agent UUID for both. Never use a synthetic workflow ID as a
memory owner.

Memory chat events are projected in `src-tauri/src/commands/chat.rs` and rendered
by the shared transcript row. Do not archive them as conversation records: their
source and retention lifecycle is `memory.db`.

Workflow mutation is routed through the `memory_commit` engine node. Its payload
is a `MemoryCommitBatch`; the live executor calls `MemoryStore::commit_batch`.
`MemoryCommitRequest.agent_id` is rendered from the canonical
`{{trigger.output.agent_id}}` invocation value by the engine. No other template
is accepted. That rendered value is not authority: the live executor also
requires an immutable invocation principal supplied by a trusted session-close
launch or by a managed CLI process whose launch-scoped capability the desktop
validated. The principal is persisted in `invocation.json` for approval resume.
The executor must reject unauthenticated commits and any request or batch whose
`agent_id` differs from that principal before opening the memory store.
Do not add implicit memory writes to task, shell, script, state, or notification
nodes. Generic session-close invokers are persisted under
`library/session-close-invokers.json` and launch the ordinary run path.
Use their generic `require_archive` option for workflows whose inputs only make
sense when a durable closing archive exists; a skipped run consumes no provider
quota.

Mutate session-close invokers only through
`wardian_core::session_close::mutate_invokers`; it holds the adjacent `.lock`
file across reload, mutation, and atomic replacement. A session-close context
owns a unique `boundary_id`, and lifecycle code must invoke workflows only after
the clear operation has committed successfully.

The CLI treats a non-empty `WARDIAN_SESSION_ID` as a managed principal. Managed
callers must also present a matching `WARDIAN_MEMORY_CAPABILITY`, which the
runtime issues per provider process and stores only as a SHA-256 hash in
`memory.db`. Concurrent processes may hold capabilities for the same agent.
Each `ActiveAgent` retains its exact capability lease and revokes it when
Wardian terminates, replaces, or drops that runtime. The PTY reader must not own
a revoker: reader/broker failure is not proof that the provider child exited.
Revoking one process does not invalidate another concurrent process.
Managed callers may resolve and mutate only themselves; changing an environment
variable alone does not authorize another identity. Keep persisted `state.db` resolution
available when the desktop control endpoint is offline, but reject unknown or
ambiguous names rather than treating them as memory owner IDs.

When changing selection or rendering, increment the memory budget-policy version
so resumed providers recover with a full changed fingerprint. An unchanged
resume still receives the active checkpoint: an injection audit does not prove
the prior process submitted a provider turn. Preserve these tests: revision
lifecycle, agent/workspace isolation, integrity exclusion, fresh/resume delta,
bounded rendering, batch atomicity/idempotency, provider instruction delivery,
compact event disclosure, and live temporary-agent recall.

Do not deduplicate injection rows by fingerprint. Fingerprints describe memory
content; injection IDs are delivery receipts. A successful non-empty headless
run records after its zero exit, while an interactive runtime records only after
provider-ready evidence. Failed or pre-readiness launches must not advance the
checkpoint. OpenCode uses its provider-owned ready title as that evidence because
its raw PTY stream has no stable compose marker. Every accepted delivery appends
a distinct row and `loaded` event.

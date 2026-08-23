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

Memory chat events are projected in `src-tauri/src/commands/chat.rs` and rendered
by the shared transcript row. Do not archive them as conversation records: their
source and retention lifecycle is `memory.db`.

Workflow mutation is routed through the `memory_commit` engine node. Its payload
is a `MemoryCommitBatch`; the live executor calls `MemoryStore::commit_batch`.
Do not add implicit memory writes to task, shell, script, state, or notification
nodes. Generic session-close invokers are persisted under
`library/session-close-invokers.json` and launch the ordinary run path.
Use their generic `require_archive` option for workflows whose inputs only make
sense when a durable closing archive exists; a skipped run consumes no provider
quota.

When changing selection or rendering, increment the memory budget-policy version
so resumed providers recover with a full changed fingerprint. An unchanged
resume still receives the active checkpoint: an injection audit does not prove
the prior process submitted a provider turn. Preserve these tests: revision
lifecycle, agent/workspace isolation, integrity exclusion, fresh/resume delta,
bounded rendering, batch atomicity/idempotency, provider instruction delivery,
compact event disclosure, and live temporary-agent recall.

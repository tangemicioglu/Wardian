# Headless Provider Execution Contract

**Date:** 2026-07-26
**Status:** Implemented

## Context

Temporary-provider workflow nodes and messages to off agents execute provider
CLIs without a visible persistent terminal. Those paths must be held to the
same launch, execution, and readable-output contract as an active agent
receiving `wardian send`.

Issue #680 exposed one breach of that contract: Wardian placed Codex
`exec`-specific flags before the `exec` subcommand. Current Codex rejects that
command shape before it can run a workflow. Other provider paths also made
workflow success depend on optional session-discovery artifacts instead of the
provider's actual response text.

## Decision

Wardian maintains a real-provider execution matrix for Codex, Claude,
OpenCode, and Antigravity. Deprecated Gemini is excluded from this maintained
matrix.

For headless execution:

- Codex global flags precede `exec`; `--skip-git-repo-check` and `--ephemeral`
  follow `exec` or `exec resume`. Temporary workflows default the Git-repository
  bypass on, so their workspace need not be a Git checkout.
- Every provider normalizes a successful run into a readable response used by
  the workflow node output. OpenCode and Antigravity may return an absent
  session identity when their CLI output or cache has no exact mapping; that is
  not an execution failure when a readable response is available.
- Exact provider session identities remain mandatory when Wardian needs to
  resume or manage a persistent agent. The relaxed workflow behavior does not
  infer, invent, or substitute a session identifier.
- An ordinary `wardian send` to an off or errored agent uses the same headless
  process transport by default. Wardian takes an agent-level conversation
  lease for both resumed and fresh runs, exposing the purple `Headless` status
  while the process owns the target. A resumed run leases its provider session;
  a fresh run leases the agent without inventing a provider session identity.
  Its normalized response is retained in `agent watch` and as a durable
  input/assistant exchange in the conversation archive.
- A workflow targeting a registered agent uses that same agent-level lease for
  both background-resume and background-fresh routes. A fresh route retains a
  workflow-scoped provider session identity, but it cannot overlap a lifecycle
  transition, another fresh run, or a resumed turn for the registered agent.
  Truly ephemeral provider workers have no registered-agent lease.
- A headless process is bounded by the request timeout (at most 15 minutes).
  It is killed on cancellation or timeout as a complete process tree: Windows
  terminates the root with `taskkill /T`, and Unix providers run in a dedicated
  process group. The cancellation guard runs before the direct Tokio child is
  dropped, so shell wrappers cannot leave a provider descendant using the
  conversation after Wardian releases its lease. The process renews an active
  lease every minute and releases that exact acquisition on completion. If
  cleanup initially fails after the provider has applied the prompt, delivery
  remains `provider_applied` and the guard makes one best-effort follow-up
  release as the request unwinds; lease expiry still bounds a stale cleanup
  without inviting a caller retry.
- Headless lease transitions are retained as `Headless` and underlying-status
  observations in `agent watch` and as roster status events. An offline agent
  continues to display purple `Headless` even though its persisted lifecycle
  setting remains off. A separate fresh workflow does not replace a live
  agent's normal status with `Headless`. A single-target `send --wait-until idle` treats the
  completed headless delivery event (`provider_applied`) as the completion
  condition while preserving the target's truthful offline status.
- Agent lifecycle operations and in-process headless runs share a per-agent
  gate. Every path claims the persisted lease before waiting for that local
  gate. Resume, clear, pause, and remove use a `lifecycle_transition` lease
  before mutating or launching a runtime, so a separate Wardian process cannot
  overlap the same saved provider conversation. Lease-file mutation is guarded
  by an OS-level lock, not only an in-process mutex. Lifecycle transitions
  renew their lease every minute and synchronously fence every destructive or
  replacement boundary if renewal loses ownership. Each lease acquisition has
  a unique token, so an expired workflow attempt cannot renew or release a
  later attempt that reused its run/node owner ID. A conflicting headless
  lease rejects the lifecycle action before it changes the agent. Runtime
  replacement installs a new status incarnation before stopping the prior
  runtime; durable status writes and later UI side effects verify that
  incarnation so late PTY events cannot restore stale readiness. Headless
  response, watch, and archive writes are bound to the resolved active-agent
  incarnation and are discarded if that agent was cleared, resumed, replaced,
  or removed.
- Every headless provider execution holds a shared, Wardian-home-wide execution
  lock while it can use a workspace. Workflow engine drives hold it for their
  duration; direct offline send and ask hold it for their headless delivery. A
  workflow can dispatch a registered agent or temporary provider in a workspace
  different from the workflow's own run workspace, so deletion cannot safely
  infer one narrow path to lock. Deleting any managed worktree therefore first
  requires the exclusive counterpart and is rejected while any headless
  provider execution is active rather than removing files beneath it. Deletion
  claims that counterpart before it snapshots worktree membership, so an agent
  assignment cannot appear between its safety check and removal. The lock is
  both process-local and OS-backed so it covers another Wardian process sharing
  the same home.
- Lease acquisition is authoritative. A competing `queue-if-busy` send that
  loses the acquisition race is mailbox-queued with `conversation_leased`
  instead of failing. Each successfully completed target archives its exchange
  before a mixed-target send reports any other target failure.
- Explicit `mailbox-only` sends remain deferred, and provider slash commands
  remain deferred while their target has no interactive surface.

The native E2E suite has two opt-in real-account tests and one deterministic
mock coverage case:

1. `provider-headless-workflow-real-native.test.mjs` launches a temporary
   provider from a workflow and asserts a completed node with readable output.
   Its Codex leg uses an isolated non-Git workspace.
2. `provider-delivery-real-native.test.mjs` launches a real persistent agent,
   queues a `wardian send`, confirms its delivery transition, and finds the
   reply through `wardian agent watch`. Its isolated test home marks only the
   chosen Codex workspace trusted; it does not change a user's Codex settings
   or approval policy.
3. `cli-shared-state-native.test.mjs` creates an off mock agent, sends it a
   normal message, observes its `Headless` telemetry status, and verifies the
   normalized response through `agent watch`.

The two real-account tests require an explicit environment switch because they
invoke provider accounts. The default test selection is the complete
four-provider matrix; local single-provider debugging needs an explicit
partial-matrix flag.

## Consequences

- **Positive:** Workflow and cross-agent messaging now have a concrete,
  provider-by-provider runtime contract rather than only mock or argument-level
  coverage.
- **Positive:** Codex temporary workflows work in ordinary non-Git folders,
  while interactive launch arguments retain their existing placement rules.
- **Positive:** Provider-specific session lookup remains useful metadata rather
  than an unnecessary condition for preserving a completed workflow response.
- **Positive:** Headless messages have a bounded, observable lifecycle even
  when the target has no saved provider conversation, and retain durable
  response history after the app restarts.
- **Trade-off:** Real-provider tests consume authenticated provider capacity and
  remain opt-in; CI continues to rely on deterministic unit and mock/native
  coverage unless credentials are deliberately supplied.

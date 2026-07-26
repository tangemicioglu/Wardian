# Headless Provider Execution Contract

**Date:** 2026-07-26
**Status:** Implemented

## Context

Temporary-provider workflow nodes execute provider CLIs without a persistent
agent session. That path must be held to the same launch, execution, and
readable-output contract as an active agent receiving `wardian send`.

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

The native E2E suite has two opt-in real-account tests:

1. `provider-headless-workflow-real-native.test.mjs` launches a temporary
   provider from a workflow and asserts a completed node with readable output.
   Its Codex leg uses an isolated non-Git workspace.
2. `provider-delivery-real-native.test.mjs` launches a real persistent agent,
   queues a `wardian send`, confirms its delivery transition, and finds the
   reply through `wardian agent watch`. Its isolated test home marks only the
   chosen Codex workspace trusted; it does not change a user's Codex settings
   or approval policy.

Both tests require an explicit environment switch because they invoke real
provider accounts. The default test selection is the complete four-provider
matrix; local single-provider debugging needs an explicit partial-matrix flag.

## Consequences

- **Positive:** Workflow and cross-agent messaging now have a concrete,
  provider-by-provider runtime contract rather than only mock or argument-level
  coverage.
- **Positive:** Codex temporary workflows work in ordinary non-Git folders,
  while interactive launch arguments retain their existing placement rules.
- **Positive:** Provider-specific session lookup remains useful metadata rather
  than an unnecessary condition for preserving a completed workflow response.
- **Trade-off:** Real-provider tests consume authenticated provider capacity and
  remain opt-in; CI continues to rely on deterministic unit and mock/native
  coverage unless credentials are deliberately supplied.

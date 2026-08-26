# Chat tool command normalization

## Context

The shared desktop and remote-mobile chat surfaces render the normalized
`AgentChatEvent` stream. Recent provider payloads can name a shell tool `exec`
and carry its invocation under `cmd` rather than `command`. Successful tool
results can also contain transport text such as `Script completed`. If those
provider details remain unnormalized, mobile work logs show `exec` and
`output` instead of the command an operator needs to inspect.

## Decision

- Normalize `command`, `cmd`, `CommandLine`, and `script` tool-input fields into
  `AgentChatEvent.command` at the provider boundary.
- Treat `exec`, `output`, and script-completion labels as low-signal provider
  metadata when deciding what a tool row is called.
- Merge successful results containing only script-completion boilerplate into
  their tool call without adding that boilerplate to visible output.
- Keep meaningful output, failures, approvals, changed-file evidence, and
  structured results visible.

## Invariants

1. A shell invocation remains visible as its concrete command on desktop and
   remote mobile, regardless of the provider's command-key alias.
2. Provider-only labels such as `exec`, `output`, and `Script completed` do not
   become standalone work-log content, even when a provider omits explicit
   success metadata. Meaningful output remains visible.
3. A successful result with meaningful output is not suppressed.
4. The backend emits one canonical command field for all clients.

## Verification

- Rust provider-normalization tests cover Codex `exec`/`cmd` and Pi tool-input
  command extraction.
- Frontend presentation tests cover the `exec` plus `output`/`Script
  completed` sequence.
- The remote mobile route test verifies the command remains visible while the
  provider labels and completion text remain absent.

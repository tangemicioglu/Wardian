# Provider Advanced Configuration Contract

- **Status:** Implemented
- **Date:** 2026-08-12

## Context

Agent Advanced Settings mixed interactive provider options, headless-only
options, deprecated aliases, and controls that some provider adapters ignored.
This allowed Wardian to persist a plausible configuration that either failed
at provider startup or never affected the launched process.

## Decision

Treat the provider's current CLI contract as the authority for advanced agent
configuration. A visible field must satisfy all of these conditions:

1. The selected provider supports the option.
2. Wardian passes the option in the execution mode named by the UI.
3. Enumerated values are limited to values accepted by that provider.
4. Wardian owns transport flags such as structured headless output format; an
   agent setting cannot override them.

Headless-only options remain agent configuration because background delivery
and workflow execution use the assigned agent's provider profile. The UI labels
these options as headless so users do not expect them to change the interactive
terminal.

## Provider Contract

| Provider | Interactive options | Headless-only options |
| --- | --- | --- |
| Claude | permission mode, available tools, tools allowed without prompting, denied tools, MCP config, strict MCP config, appended system prompt | maximum turns |
| Gemini | sandbox, approval mode, policy and admin-policy paths, allowed MCP servers, extensions, screen reader | none; Wardian owns output format |
| Codex | sandbox, approval policy, profile, live search, per-agent approval/sandbox bypass | skip Git repository check, ephemeral session |
| Antigravity | sandbox, permission bypass, agent mode, named agent | print timeout |
| OpenCode | named agent, auto-approval | none |

Shared debug and include-directory controls render only when the selected
provider adapter consumes them. Custom arguments remain the explicit escape
hatch for provider flags Wardian does not model.

## Compatibility

- Legacy Claude `auto-accept` is translated to `acceptEdits`.
- Legacy Claude `default` is translated to the current `manual` mode.
- Legacy Gemini `yolo: true` is translated to `--approval-mode yolo`; Wardian
  no longer emits the deprecated `--yolo` alias.
- Legacy Gemini ACP and output-format fields remain readable but are not passed
  to interactive launches. ACP changes the process protocol, and Wardian owns
  headless output framing.
- Gemini sandbox is a current CLI option, but its runtime requires Docker or
  Podman. Wardian surfaces that prerequisite next to the per-agent control.
- Removed Codex `on-failure` values normalize to Wardian's `on-request`
  default because current Codex versions no longer accept that policy.

## Verification

Provider adapter tests assert exact argument placement for interactive and
headless launches. Frontend tests assert that obsolete or unsupported controls
are absent and that edits update the selected nested `provider_config` variant.
Native E2E uses provider shims to capture the actual child-process argument
vector after a per-agent config is saved. It covers both offline headless
delivery and PTY resume for every supported provider without using provider
accounts or quota.

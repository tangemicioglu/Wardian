# Provider Model and Effort Selection

- **Status:** Implemented
- **Date:** 2026-08-02

## Context

Model identifiers and reasoning controls change independently across provider
versions and accounts. A static list in the frontend would become stale, offer
models that an account cannot use, or suggest an effort level a selected model
does not support.

Wardian already persists an agent-level `model` override, but it was only a
free-text field in Advanced Settings. The setting needed to become a primary
agent property so spawning, configuration, and chat share one value.

## Decision

Wardian exposes a provider-owned model catalogue through
`list_provider_model_catalog` and renders one shared selector in Spawn Agent,
Configure Agent, and the chat composer.

- Codex runs `codex debug models`, whose JSON catalogue includes the models and
  their supported reasoning levels.
- OpenCode runs `opencode models --refresh`, so its catalogue follows the
  configured provider accounts and Models.dev refresh. Older versions fall
  back to `opencode models` without claiming their list was refreshed.
- Claude and Gemini expose their stable provider-maintained aliases. The
  aliases resolve to the installed provider's current mappings instead of
  pinning a Wardian release to a dated concrete model ID.
- Antigravity uses its `models` command when available. Older installations
  that do not expose it leave model controls unavailable rather than receiving
  unsupported launch arguments.

Catalogues are cached in-process for five minutes, refresh automatically on
that cadence while the selector is open, and can be explicitly refreshed. A
failed refresh retains the last known-good catalogue with a visible error;
Wardian never silently substitutes a guessed model list.

`AgentConfig.model` remains the compatible persisted model field. Effort is a
provider-owned configuration value named `reasoning_effort` for providers with
a stable Wardian launch mapping:

- Codex: `-c model_reasoning_effort="<effort>"`
- Claude: `--effort <effort>`
- Antigravity: `--effort <effort>` when exposed by its discovered model flow

Gemini and OpenCode do not currently expose one provider-independent
launch-time effort flag, so Wardian deliberately hides the effort selector for
them. Their model selection remains available.

## Behavior

Changing a selection in Spawn Agent becomes part of the spawn override.
Changing it in Configure Agent persists the same fields and follows the
existing restart-required behavior for launch arguments. Changing it in chat
uses the focused `update_agent_model_selection` command and states that the
selection applies when the agent next starts or restarts. Wardian does not
interrupt an active provider turn or claim that a session changed in place.

## Compatibility

Older persisted agents deserialize without an effort value. A discovered
catalogue can omit a saved model; the selector retains it as a saved value
rather than overwriting it. Provider discovery checks the installed executable
before advertising its controls, and provider command timeouts fail closed.

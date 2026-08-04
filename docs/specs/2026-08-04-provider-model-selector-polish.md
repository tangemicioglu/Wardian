# Provider Model Selector Polish

- **Status:** Implemented
- **Date:** 2026-08-04

The agent model and effort selector refreshes provider catalogues automatically
while it is open. It does not expose a manual refresh control, and it does not
show provider-alias explanatory copy in the configuration surface.

Claude's stable aliases include `sonnet`, `opus`, `haiku`, and `fable`.

Codex's `codex-auto-review` entry is an internal approval-review model, not a
user-facing agent model, so Wardian excludes it from the selectable catalogue.

On Windows, OpenCode discovery preserves the resolved npm shim path so native
provider commands launch the installed `opencode.cmd` instead of relying on
shell-only command lookup.

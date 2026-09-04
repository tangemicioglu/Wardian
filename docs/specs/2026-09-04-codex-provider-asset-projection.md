# Codex Provider Asset Projection

* **Status:** Implemented
* **Date:** 2026-09-04

## Context

Wardian gives each Codex agent an isolated `CODEX_HOME`. The existing
projection copied profile credentials and merged configuration, but omitted
the native marketplace catalogs and plugin implementation cache. As a result,
the projected configuration could mention bundled plugins while Codex could
not resolve them, and provider-generated MCP entries could retain stale
runtime paths.

## Decision

During every Codex habitat refresh, Wardian projects these provider-owned asset
roots from the native Codex home into the agent home as directory links:

- `.tmp/bundled-marketplaces`
- `.tmp/plugins`
- `plugins/cache`

The `.tmp/plugins.sha` marker is copied inward. Agent databases, sessions,
history, indexes, logs, credentials, and configuration remain governed by the
existing isolation rules. Marketplace, provider MCP, and provider hook-state
records from the native config refresh matching entries; agent-only MCP and
plugin entries remain available.
Native-home paths embedded in the reconciled config are rewritten to the
projected home where the agent-local path is required.

This is intentionally directory-based rather than a list of named plugins, so
newly installed or newly bundled provider plugins follow the same projection
path without a code change.

## Verification

Unit coverage proves that all three asset roots are projected, the marker is
copied, private SQLite state is not projected, stale provider MCP entries are
removed, current native runtime entries replace stale values, and custom
agent-only MCP entries remain.

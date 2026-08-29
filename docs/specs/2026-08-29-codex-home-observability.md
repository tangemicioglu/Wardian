# Codex Home Observability

* **Status:** Implemented
* **Date:** 2026-08-29
* **Issue:** #1065

## Context

Wardian gives each agent an isolated `CODEX_HOME` under its habitat. That
isolation protects provider databases and agent-local configuration, but it
also hides native Codex session rollouts and the provider's index/history
records from the user's normal Codex home. Codex CLI `0.150.1` stores these
surfaces in `sessions/`, `session_index.jsonl`, and `history.jsonl`.

## Contract

### Session rollouts

An agent home's `sessions/` is a directory projection of the user's native
Codex `sessions/` directory. Windows uses a directory junction; POSIX systems
use their directory-link equivalent. Rollout filenames are preserved exactly.

When an existing agent has local sessions, Wardian copies missing files into the
native tree before staging the local directory and installing the projection.
Identical files are retained, while a same-name/different-content conflict
fails closed without overwriting either copy. A failed link attempt restores
the local directory, so session visibility is best effort and cannot prevent
provider startup. Repeated synchronization is idempotent.

### Central JSONL indexes

The provider continues writing `history.jsonl` and `session_index.jsonl` in its
agent-local home. Wardian observes complete newline-terminated JSON records and
appends records absent from the native central file. Malformed and partial
records are deferred or ignored, and repeated observations do not duplicate a
record. A process mutex plus an exclusive lock file serializes Wardian's
central writers.

### Isolation and directionality

SQLite databases (`state_5.sqlite*`, `logs_2.sqlite*`, and other Codex
databases) remain per-agent. They are not junctioned, copied, or hardlinked,
because WAL/SHM files are path-sensitive. Runtime logs, caches, and temporary
files remain local as well.

`auth.json` and `cap_sid` are projected from the native home into each agent
home only. No credentials or capability files are copied from an agent home
back to the native home. Configuration remains an agent-local overlay over
shared defaults, including agent-owned workspace trust and local overrides.

### Bootstrap and ongoing observation

Temporary headless/bootstrap homes use the same projection rules. Bootstrap
migration skips central index files after they have been observed and detects
when bootstrap and final `sessions/` paths resolve to the same directory, so it
does not recurse through a junction. Telemetry periodically observes agent
index files so updates made after startup become visible without reopening the
agent.

## Consequences

Users can inspect native Codex sessions and index/history records while Wardian
retains SQLite and mutable runtime isolation. Session projection depends on
filesystem link permissions; a host that denies directory links still receives
working local-only sessions, with the visibility limitation recorded in debug
logs. The native Codex layout is an external dependency and should be
revalidated when upgrading beyond `0.150.1`.

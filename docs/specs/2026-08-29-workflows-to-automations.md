# Workflows to Automations Rename

## Decision

Wardian's user-facing process feature is named **Automations**. This is a
total vocabulary migration: the CLI, Tauri commands, Rust modules, frontend
surfaces, generated node contracts, documentation, bundled resources, and
canonical on-disk directories use `automation`.

There is no `workflow` compatibility command, API alias, duplicate component,
or dual read/write path. GitHub Actions terminology and the repository's
historical specs remain unchanged because they describe a separate system or
record prior decisions.

## Persisted-state contract

Existing installations are migrated before the desktop app or CLI reads
automation state:

- `library/workflows/` becomes `library/automations/`;
- `logs/workflows/` becomes `logs/automations/`;
- blueprint front matter changes only the structured `sub_workflow` node type
  and its `workflow` field to their `sub_automation` and `automation`
  equivalents; Markdown prose and authored values are preserved;
- `library/schedules.json` keeps schedule and blueprint identities stable, so
  the 22 live schedule records in the reference installation remain the same
  records.
- Desktop startup gates automation-dependent initialization when this migration
  fails; the CLI reports the failure instead of reading a partial layout.
- Persisted workbench open and recently-closed surfaces named `workflows` are
  rewritten to `automations` and saved as a successor document revision before
  the frontend resolves the surface registry.
- Persisted Inbox records are normalized from the old completion, failure, and
  approval field names to canonical automation fields while retaining ids,
  timestamps, read state, approval payloads, and dismissal markers. The
  normalized queue is written back for retry-safe desktop and remote reads.

The migration is retryable. A missing destination is moved with a filesystem
rename. If a destination already exists, entries are reconciled recursively;
identical files are deduplicated, while conflicting entries fail without
overwriting either copy. The legacy directory is removed only after all of its
entries have been moved or proven identical. This makes an interrupted or
partially completed migration safe to resume. Blueprint field rewriting
recognizes UTF-8 BOM and LF or CRLF front matter and preserves the source
newline style.

## Exclusions

The migration does not rewrite `.github/workflows/`, the historical specs in
`docs/specs/`, or historical `e2e/screenshots/workflow-monitor-*` evidence.
General English and GitHub Actions uses of “workflow” are not product
vocabulary and are left intact.

## Verification requirements

The migration must be tested with a populated copied home, including blueprint
files, run directories, and schedules. Tests must cover a first migration, a
retry after partial migration, identical duplicate entries, conflicting
entries, BOM/CRLF blueprints, renamed workbench surfaces, and normalized Inbox
records. The generated TypeScript schema and node reference must be produced
from the Rust registry and checked for drift.

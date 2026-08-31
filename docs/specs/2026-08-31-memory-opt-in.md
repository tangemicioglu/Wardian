# Agent memory opt-in

Status: Implemented

## Decision

Wardian's provider-integrated agent memory is experimental and is disabled by
default. Users can opt in globally through **Settings > Agent Runtime > Agent
memory**.

The setting gates the behavior that changes a provider process: startup recall,
the direct-retention instructions, the managed memory capability, and
`memory_commit` automation execution. Existing records and explicit desktop
operator inspection remain available when the integration is disabled. The
optional Memory Consolidation automation remains independently disabled until a
user configures and enables it.

The setting is read at provider launch. Enabling or disabling it therefore
applies to new and restarted provider processes; an already-running process
retains its launch environment until it exits.

Missing or unreadable settings fail closed. This prevents an unavailable
settings file from enabling an experimental provider capability accidentally.

## Compatibility

The app settings document remains schema version 2. Older files omit the new
override and resolve to `memory_enabled: false`. An explicit `true` override is
preserved as a sparse setting. The frontend's local preference migration also
defaults the new control to disabled.

## Verification

- Rust app-settings tests cover the disabled default and enabled round trip.
- Frontend store tests cover persistence and local migration; Settings tests
  cover the disabled initial control and explicit opt-in.
- Provider launch code gates interactive and headless memory setup and
  capabilities; automation execution rejects `memory_commit` while disabled.

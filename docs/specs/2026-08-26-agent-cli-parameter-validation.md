# Spec: Agent CLI Parameter Validation

## Problem

Agent creation requests previously allowed `agent spawn` to pass an arbitrary
class name into the live agent state. If the name was absent from the class
registry, the new agent could be created without the role instructions that
define that class. The same creation boundary also accepted provider casing
that later changed provider dispatch behavior, and cloning could preserve an
orphaned class or provider from an existing record.

## Decision

Validate and canonicalize finite creation inputs before any provider session
bootstrap, name reservation, profile copy, or roster mutation:

- Classes must resolve case-insensitively against the initialized class
  registry and are stored using the registry's canonical name.
- Providers must resolve case-insensitively against the supported provider
  registry and are stored using the lowercase provider ID.
- `agent spawn` performs this validation in both the control request boundary
  and the command implementation, so CLI and desktop callers share the same
  invariant. Unknown classes are reported as not found by the CLI control
  path.
- `agent clone` validates the source and optional override class/provider,
  preventing a new agent from inheriting an orphaned finite parameter.

Model IDs and reasoning efforts remain provider-discovered values. They are
not hard-coded into the Wardian CLI because their valid set can change with
the installed provider; provider-specific live selection remains responsible
for validating those values when a provider exposes that capability.

Existing agent names and workspace paths retain their established validation
rules. `agent update --class` already enforced the class registry invariant;
it now shares the same class lookup helper as creation.

## Invariants

1. A successful spawn or clone has a registered, canonical agent class.
2. A successful spawn or clone has a supported, canonical provider ID.
3. Invalid finite creation parameters fail before state or provider mutation.
4. Class lookup remains case-insensitive for compatibility with existing CLI
   usage.

## Verification

- Core class lookup tests cover trimming, case-insensitive matching, and
  unknown names.
- Tauri command tests cover canonical class/provider resolution and rejection
  of unknown values.
- Existing CLI and Tauri agent lifecycle tests cover request construction,
  update canonicalization, name validation, and workspace validation.

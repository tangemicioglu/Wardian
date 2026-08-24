# Phase 2 — Reliable startup recall

## Outcome

Every new provider process receives Wardian-owned memory before it begins useful work, regardless of provider-native history behavior. Users can see exactly what was loaded.

## Compilation

Before spawn, Wardian resolves the agent and active workspace, reads eligible active revisions, and compiles two Markdown sections: `Stable memory` and `Current state`. Each entry carries a short stable identifier, scope, and verification timestamp. Current-state entries older than the configured stale threshold are labeled `stale`; they are not deleted.

Compilation uses a deterministic character budget and stable ordering. Agent-wide stable records, workspace stable records, and workspace current state are admitted in that order. If the budget omits records, the brief reports the omitted count. The fingerprint is SHA-256 over the ordered revision IDs, revisions, and normalized text.

For a fresh conversation, Wardian injects the full compiled brief. For a resumed conversation in a new provider process, Wardian compares against the latest injection for that provider conversation key and injects added, changed, or removed revision deltas. If no prior injection exists, it injects the full brief.

## Provider-neutral delivery

The brief is projected into the generated Wardian habitat instructions, not a user-owned `AGENTS.md`. Providers that run in the habitat consume it there. Providers that run in the real workspace receive the habitat through their supported include-directory/system-instruction path. Spawn fails closed if memory compilation succeeds but the selected provider cannot receive the generated instructions; empty recall is not an error.

The projection includes the direct-retention protocol and current brief. SQLite remains the source of truth and the generated file is disposable.

## Observability

Successful writes append `Memory saved`, `Memory updated`, or `Memory removed` events scoped to the agent. A non-empty startup injection appends one collapsed `Memory loaded` event with counts and fingerprint. Expanding it shows the exact context supplied to the provider. Empty recall creates no chat row, while diagnostic APIs report an empty result.

The chat transcript merges memory events with provider and conversation events by timestamp and stable event ID. Memory rows use semantic theme variables and render identically in local and remote chat views.

## Tests

- Fresh process receives full stable/current brief.
- Resumed provider process receives only additions, updates, and removals since its prior fingerprint.
- A second agent and a second workspace receive no leaked state.
- Empty recall yields no synthetic prompt or chat event.
- Context budget ordering and omitted count are deterministic.
- Exact injected text and fingerprint round-trip through the memory event row.
- Provider argument/instruction tests cover Codex, Claude, Gemini, OpenCode, Antigravity, and mock.
- Native E2E proves injection occurs before the first provider turn.

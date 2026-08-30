# Agent registry reconciliation

## Decision

Agent restoration must never rewrite `settings/state.json` without retaining
every persisted agent. If preparation of one configuration fails, Wardian
keeps an error placeholder in the live roster and records the failure in
`settings/agent-registry-quarantine.jsonl`.

Topology reconciliation may still remove dangling references, but it records
each unknown session ID, timestamp, and reason in the same append-only
quarantine log before pruning. This preserves evidence without restoring an
invalid graph edge.

The control-plane agent list also includes durable database agents absent from
the live map. These entries use the persisted status source, so operators can
distinguish a healthy persisted record from a live agent and investigate the
inconsistency without losing the record.

## Verification contract

Regression tests cover JSONL quarantine records and the existing restoration
snapshot path covers preservation of all live configurations. Do not delete or
recreate an agent to test this behavior; use a temporary fixture or a
throwaway identity only.

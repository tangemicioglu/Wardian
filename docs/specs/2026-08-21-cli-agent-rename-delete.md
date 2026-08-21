# CLI agent rename and deletion

## Decision

Wardian exposes the existing live agent lifecycle through two CLI commands:

- `wardian agent rename <target> <new-name>` changes an agent's name in live
  state and persisted metadata without restarting its provider. Target
  resolution for `send`, `ask`, and other commands sees the new name after the
  control request succeeds. Names remain unique and use the existing agent
  name character rules.
- `wardian agent delete <target> --confirm <current-name>` permanently removes
  a stopped agent. The confirmation must exactly match the current name, and
  deletion refuses while the provider runtime is attached. An operator must
  pause the agent first; the legacy confirmed `agent kill` path remains
  available when force-removal is intended.

Deletion cascades Wardian-owned state: the roster/database record, private
agent habitat, delivery references, snapshots, and saved session history are
removed. It never removes project workspace files. This preserves the existing
desktop Delete Agent ownership boundary and makes the history decision
explicit rather than leaving an orphaned agent record.

The existing `agent kill --confirm` command remains available for compatibility
with the desktop lifecycle path. New automation should use `agent delete` when
it wants the stopped-agent guard and exact-name confirmation.

## Rationale

The CLI must use Wardian's control endpoint because the running application owns
the authoritative in-memory roster and provider lifecycle. Editing
`settings/state.json` or `state.db` directly would leave the live target index,
provider runtime, and UI out of sync. A rename is therefore a normal live
configuration operation, while deletion is a guarded lifecycle transition.

# CLI agent rename and deletion

## Decision

Wardian exposes the existing live agent lifecycle through two CLI commands:

- `wardian agent rename <target> <new-name>` changes an agent's name in live
  state and persisted metadata without restarting its provider. Target
  resolution for `send`, `ask`, and other commands sees the new name after the
  control request succeeds. Names remain unique and use the existing agent
  name character rules.
- `wardian agent delete <target> --confirm <current-name>` permanently removes
  an agent. The confirmation must exactly match the current name. Deletion
  refuses while the provider runtime is attached unless `--force` is supplied;
  force controls provider termination and does not weaken confirmation.

Deletion cascades Wardian-owned state: the roster/database record, private
agent habitat, delivery references, snapshots, and saved session history are
removed. It never removes project workspace files. This preserves the existing
desktop Delete Agent ownership boundary and makes the history decision
explicit rather than leaving an orphaned agent record.

There is one destructive CLI lifecycle operation. New automation should use
`agent delete`; `--force` is the explicit provider-termination option.

## Rationale

The CLI must use Wardian's control endpoint because the running application owns
the authoritative in-memory roster and provider lifecycle. Editing
`settings/state.json` or `state.db` directly would leave the live target index,
provider runtime, and UI out of sync. A rename is therefore a normal live
configuration operation, while deletion is a guarded lifecycle transition.

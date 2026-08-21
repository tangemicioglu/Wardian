# Agent lifecycle safety

## Decision

Wardian uses distinct lifecycle terms for distinct data effects:

- **Restart Session** restarts the provider while retaining the agent identity,
  habitat, and saved session history.
- **New Session** creates a new provider context while retaining the Wardian
  agent, habitat, and saved history.
- **Delete Agent** permanently removes the Wardian agent, its habitat, and its
  session history. It does not remove project workspace files.

Renaming uses `wardian agent rename <target> <new-name>` and updates the live
identity without restarting the provider. The new name is immediately used by
target resolution. The CLI exposes the safe reclass sequence as
`wardian agent update … --class …` followed by `wardian agent restart …`.
`wardian agent delete <target> --confirm <current-name>` is the explicit
destructive path: it refuses an attached provider runtime and requires the
operator to echo the exact current name. The older `agent kill --confirm`
syntax remains a force-removal compatibility path.

## Rationale

Changing an agent class requires a provider restart to apply updated instructions.
It must not require deleting the agent or its accumulated Wardian state. Explicit
terms and a target-name confirmation make the irreversible boundary visible to
both operators and automation. Deletion deliberately cascades Wardian-owned
agent metadata, private habitat files, and saved session history; project
workspace files are outside the agent's ownership and remain untouched.

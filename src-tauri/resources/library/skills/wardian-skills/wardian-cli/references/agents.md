# Agents

## Inspect Agents

Begin a Wardian coordination task by checking yourself and the relevant roster:

```bash
wardian agent
wardian agent list --scope all --fields name,uuid,description,class,provider,workspace,status,status_source
wardian agent doctor reviewer-a1
```

Use `wardian agent` or `wardian agent show` without a target only inside a
Wardian-managed terminal; both need `WARDIAN_SESSION_ID`. Outside one, provide
an agent name or UUID:

```bash
wardian agent show Wardian-Codex
wardian agent show 019d331a-0500-7592-969f-8f437886f42b
```

Default listings show neighbors: manually connected peers, team-seeded edges,
or workspace-mates when no manual edge exists. Bare-name sends resolve among
neighbors before an exact global fallback. Team-seeded edges are editable; if an
edge is removed in Graph, its suppression persists until it is explicitly drawn
again.

Use scopes deliberately:

- `auto` (default) uses neighbors in a managed session, otherwise the workspace.
- `neighbors` returns self plus direct topology neighbors, with workspace
  fallback when isolated.
- `workspace` returns all agents in the current workspace.
- `all` returns every known agent; reserve it for cross-community work.

Use default indented JSON for automation. Use `--field` for one bare value,
`--fields` for a small JSON projection, `--verbose` for process and visibility
metadata, and `--pretty` only for human inspection. Use `agent doctor` to
inspect effective provider policy, `CODEX_HOME`, plugin state, launch flags,
and whether a restart is required.

```bash
wardian agent list --scope all --status idle
wardian agent list --workspace <absolute-workspace-path>
wardian agent Wardian-Codex --field status
wardian agent list --scope all --fields name,status,status_source
```

## Create And Update Agents

Mutating commands use the local control endpoint and require the desktop app
for the same `WARDIAN_HOME`:

```bash
wardian agent spawn --provider codex --class Reviewer --name reviewer-a1 --workspace <absolute-workspace-path>
wardian agent clone reviewer-a1 --name reviewer-a2
wardian agent rename reviewer-a1 release-reviewer
wardian agent update reviewer-a1 --class Reviewer --workspace <absolute-workspace-path>
wardian agent update reviewer-a1 --description "Reviews frontend release changes"
wardian agent delete reviewer-a1 --confirm reviewer-a1
wardian agent delete reviewer-a1 --confirm reviewer-a1 --force
```

Supply both `--provider` and `--class` when spawning. `clone` carries the
source agent's provider, class, workspace, and context unless overridden.

Use `agent update` instead of editing `settings/state.json`. It updates live
and persisted state together. It can update class, workspace, and the optional
purpose description atomically, regenerates class instruction includes after a
class change, and reports `updated_fields` plus `restart_required`. Description
changes do not restart the provider and do not change its instructions or
capabilities. Pass `--description ""` to clear the memo. Run
`wardian agent restart <target>` when required before relying on a new class or
workspace. Restart preserves the Wardian agent, habitat, and saved session
history. Do not use it to move a managed-worktree agent.

Use `agent rename` to change the live and persisted agent name without
restarting its provider. The new name is available to `send`, `ask`, and other
targeted commands as soon as the command succeeds; names must be unique and
may contain only letters, numbers, underscores, or hyphens.

`agent delete` is the only destructive CLI cleanup path. It always requires
`--confirm <current-agent-name>` so automation must echo the exact target name.
Without `--force`, it refuses an agent with an attached provider process; pass
`--force` when explicit provider termination is intended. Deletion removes the
Wardian agent record, its private habitat, and saved session history, but never
the project workspace files. This is a
cascade decision: the agent's Wardian-owned history is intentionally removed,
not orphaned or archived.

## Select A Provider Model And Effort

The model catalogue belongs to the installed provider and can change with the
provider version or account. Ask Wardian for the current compatible choices;
use `--refresh` when a newly installed provider version or account change
needs to be reflected immediately:

```bash
wardian agent models --provider codex --refresh
```

For routine, bounded tasks, leave model and effort on the provider default.
For complex, ambiguous, multi-step work such as architecture, deep debugging,
or security review, choose an explicit model and a higher effort only when
that catalogue lists the combination. Do not guess identifiers or apply high
effort based on an agent class alone.

```bash
wardian agent spawn --provider codex --class Reviewer --name reviewer-a1 --workspace <absolute-workspace-path> --model <model-id> --reasoning-effort <effort>
wardian agent update reviewer-a1 --model <model-id> --reasoning-effort <effort>
wardian agent restart reviewer-a1
```

`agent update` reports `restart_required` when a changed model or effort must
be applied to a running provider. Pass `--model ""` or
`--reasoning-effort ""` to return an existing agent to its provider default.
Providers that do not list an effort control accept model selection only.

## Assign A Managed Workspace

Worktrees are one way to assign a managed workspace; they are not an agent
lifecycle mechanism. List assignments when needed, then manage them only
through the official commands:

```bash
wardian agent worktree list
wardian agent worktree enable reviewer-a1 --name review-fixes
wardian agent worktree join reviewer-a1 --worktree <absolute-worktree-path-or-id>
wardian agent worktree disable reviewer-a1
```

These commands use the live desktop endpoint and clear the target session after
the assignment changes so the provider starts fresh. `disable` removes the
assignment only; it does not delete the physical worktree.

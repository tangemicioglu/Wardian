# Wardian CLI

Wardian includes a standalone `wardian` command for agents and automation to inspect, coordinate, and control known agent sessions. Wardian remains GUI/app-first for humans; the CLI is the textual control surface agents use when they need to discover themselves, coordinate peers, or ask the running app to perform live actions for the same `WARDIAN_HOME`.

Use it when an agent, script, or terminal automation needs repeatable access to Wardian state without clicking through the desktop UI.

The CLI is also Wardian's composability layer. It lets agents, scripts, and
future tools operate against the same app-owned state as the desktop UI without
screen scraping or duplicating private state. Prefer CLI/backend commands when
building repeatable automation around Wardian.

## When to Use It

- Let a managed agent identify itself with `wardian agent`.
- Send prompts or structured asks from one agent to another.
- Send an important user-facing update or an exceptional approval request to Inbox.
- Wait for an agent to reach a status or emit a marker.
- Start, stop, or inspect automations from automation.
- Read persisted teams, watchlists, and agent state when the app is not running.

## Basic Automation

1. Launch Wardian once so the CLI is installed into the Wardian bin directory.
2. Restart your terminal if `wardian` is not on `PATH`.
3. Set the same `WARDIAN_HOME` in both the app and terminal when using an isolated test home.
4. Run `wardian agent list` to confirm the CLI sees your neighbors, or `wardian agent list --scope all` to see all agents.
5. Use live-control commands only while the desktop app is running for that same home.

## Telemetry Read and Maintenance Paths

Read telemetry without changing the app-owned source cursors:

```bash
wardian telemetry summary --horizon week --dimension provider
```

Raw telemetry retention is an explicit offline write path. Stop the desktop app
and all agents first, choose the retention window, and provide a new backup
destination. The issue investigation proposes 90 days as a candidate; the CLI
requires the choice instead of silently adopting it:

```bash
wardian telemetry maintain --retain-days 90 \
  --backup "<backup-path>/state.db.before-telemetry-maintenance" \
  --quiesced --vacuum
```

PowerShell:

```powershell
wardian telemetry maintain --retain-days 90 `
  --backup "<backup-path>/state.db.before-telemetry-maintenance" `
  --quiesced --vacuum
```

The command verifies the new backup before deleting old turns, edits, and
completed activity intervals. It recomputes their hourly rollups first,
checkpoints the WAL, and runs `VACUUM` only when `--vacuum` is supplied. Rate
limit observations remain because the current rollup cannot reproduce their
history. The adjacent maintenance lock serializes this operation with schema
migration; current telemetry ingestion takes the same lock, and
`--quiesced` is still required because older app binaries and other offline
writes cannot participate in that lock.

On startup, a v4-to-v5 schema migration holds SQLite's exclusive locking mode
across its resumable copy batches. This fences older binaries that do not know
about the adjacent lock; once the migration completes or is interrupted, the
connection returns to normal locking mode and the next run resumes from its
last committed marker.

## Inbox Read and Write Paths

Agents can read the same Inbox projection that the desktop and remote surfaces
use:

```bash
wardian inbox list
wardian inbox list --unread --type action_needed,approval_request \
  --source provider_runtime,interaction_store
wardian inbox list --limit 100 --offset 100
```

PowerShell:

```powershell
wardian inbox list
wardian inbox list --unread --type action_needed,approval_request `
  --source provider_runtime,interaction_store
wardian inbox list --limit 100 --offset 100
```

`inbox list` returns schema-versioned JSON, newest first. `--type` accepts
`action_needed`, `agent_update`, `agent_completed`, `workflow_completed`,
`workflow_failed`, and `approval_request`; `--source` accepts values such as
`provider_runtime`, `interaction_store`, and `live_runtime`. Filters are
comma-separated and combine with `--unread`. A live app for the same
`WARDIAN_HOME` supplies the assembled projection; when it is unavailable, the
CLI reads persisted queue items, durable Inbox notifications, and workflow-run
checkpoints for awaiting approvals and terminal outcomes. The command is
read-only: it does not acknowledge, dismiss, or resolve an item. `--limit` is
bounded to 200 items; `--offset` pages the bounded read projection. A partial
source sets `truncated: true` and provides `next_offset`. Legacy queue items
older than seven days are excluded, matching desktop Inbox hydration.

Use the write path when an event changes the user's understanding or requires a
decision:

```bash
wardian notify update "The migration passed; one compatibility risk remains" \
  --title "Migration result"
wardian notify approval "Deploy the release" \
  --title "Deploy production" \
  --action "Run the production deployment" \
  --risk "This changes live traffic and may require rollback" \
  --choice "Deploy" \
  --choice "Do not deploy" \
  --wait
```

PowerShell:

```powershell
wardian notify update "The migration passed; one compatibility risk remains" --title "Migration result"
wardian notify approval "Deploy the release" --title "Deploy production" --action "Run the production deployment" --risk "This changes live traffic and may require rollback" --choice "Deploy" --choice "Do not deploy" --wait
```

Prefer `notify update` for a concise material result, limitation, or next-step
change; prefer `notify approval` only for irreversible, external,
security-sensitive, or materially costly actions. Keep routine progress in the
agent transcript. Both writes require a managed agent session and the running
app for the same `WARDIAN_HOME`.

## Installation

The desktop app copies the bundled CLI on startup:

- macOS/Linux command: `$HOME/.wardian/bin/wardian`
- macOS/Linux implementation binary: `$HOME/.wardian/bin/wardian-cli`
- Windows command: `%USERPROFILE%\.wardian\bin\wardian.cmd`
- Windows bash command: `%USERPROFILE%\.wardian\bin\wardian`
- Windows implementation binary: `%USERPROFILE%\.wardian\bin\wardian-cli.exe`

Wardian also attempts to add that `bin` directory to the user PATH. On Windows, Wardian installs both a `.cmd` launcher for PowerShell/cmd and an extensionless launcher for bash-family shells such as Git Bash, MSYS2, or provider shell tools that execute `bash`. Wardian-managed agent processes receive the active Wardian `bin` directory at the front of `PATH`, so shell tools inside managed sessions can resolve `wardian` without depending on the user's global shell startup files. Restart ordinary terminals after first launch if `wardian` is not found.

Set `WARDIAN_HOME` to redirect state, the CLI install location, and the live app control endpoint for tests or isolated runs.

For development, `npm run dev` uses the app debug home by default and ignores an inherited default production home from a managed agent shell. Set the same non-production `WARDIAN_HOME` before starting the dev desktop app and before running CLI commands when you want the CLI to inspect that dev app's live state.

When the desktop app is running for the same `WARDIAN_HOME`, the CLI asks the app for live agent snapshots before falling back to `state.db`. Request `status_source` when you need to know which path answered:

- `live` means the status came from the running desktop app.
- `persisted` means the CLI fell back to durable `state.db` state.

## Agent Identity

Wardian injects `WARDIAN_SESSION_ID` into managed agent processes. Inside an agent terminal, `wardian agent` resolves that session automatically.

Outside a managed agent process, pass a name or UUID:

```bash
wardian agent coder-a1
wardian agent show uuid-1
```

## Communication Topology & Scope

Wardian maintains a communication topology that shapes which agents you see and interact with by default. Your **neighbors** are determined by the graph topology: manual edges (including team-seeded edges) or your workspace-mates (if you have no manual edges).

**Why it matters:**
- `wardian agent list` shows your neighbors by default — the agents you're connected to — so you work within your context.
- `wardian send --to all` broadcasts within your neighbors, not globally.
- `wardian send --to class:Coder` resolves within your neighbors.
- Bare-name targets resolve neighbors-first; explicit UUIDs and exact names always work regardless of topology.

**Scope modes for `wardian agent list`:**
- `--scope auto` (default): neighbors when `WARDIAN_SESSION_ID` is set (inside a Wardian agent terminal), else workspace.
- `--scope neighbors`: self + direct topology neighbors (manual edges, workspace fallback when you have no manual edges).
- `--scope workspace`: all agents in your workspace.
- `--scope all`: all known agents across all workspaces.

**When to use each scope:**
- Default (`auto`): Normal agent work within your context (neighbors inside a session, workspace outside).
- `--scope neighbors`: Explicit neighbors-only listing (same as auto inside a session).
- `--scope workspace`: When you need to see all agents in your workspace regardless of edges.
- `--scope all`: Only for orchestration tasks that genuinely span multiple neighbor sets or workspaces.

When you create a team or add a team member, Wardian automatically wires up edges between all team members in the topology. These connections shape your default visibility and are completely editable through the Graph view; deleted team-seeded pairs are recorded so later seed passes do not recreate them unless you draw the connection again. See the [Graph](./graph.md) view for the visual control surface: create and delete connections, view your neighbors, and inspect the topology source at `<WARDIAN_HOME>/topology.json`.

## Graph

`wardian graph` is the CLI control surface for the communication topology — the same graph the app's Graph view edits. Agents can inspect their neighborhood without the app running; mutating the graph (`link`/`unlink`/`ignore`/`unignore`) requires it, because the running app is topology.json's sole writer (see below).

Observe:

```bash
wardian graph show                 # whole graph: agents, edges, unmapped pairs, ignored pairs
wardian graph neighbors            # my resolved neighbors with reasons (requires a session)
wardian graph neighbors coder-a1   # any agent's neighbors
wardian graph activity             # per-pair last message, open asks, unmapped flag
```

Mutate:

```bash
wardian graph link architect-a1    # inside a session: me <-> architect-a1
wardian graph unlink architect-a1
wardian graph ignore fork-coder    # durably dismiss an unmapped suggestion
wardian graph unignore fork-coder
```

- **Self-serve rule**: inside a Wardian agent terminal (`WARDIAN_SESSION_ID` set), edits must involve the calling agent — `link <other>` connects you to `<other>`; `link <a> <b>` works only if one endpoint is you. Outside a session you are the operator: `link <a> <b>` connects any pair.
- **Unmapped (ghost) pairs**: recent communication between unconnected agents. There is no separate approval verb — `link` formalizes, `ignore` dismisses.
- Mutations are idempotent: re-running reports `"changed": false` and exits 0. Errors use the standard JSON error envelope (unknown agent → exit 2, no session where one is required → exit 3, ambiguous name → exit 5, permission/self-link → exit 1, app not running → exit 6).
- Targets accept agent names or UUIDs; duplicated names require a UUID.
- Add `--pretty` to any subcommand for human-readable output instead of JSON.

Mutations are routed through the running app's control endpoint — the same
single writer the app's own Graph view uses — rather than written to
`topology.json` directly, so the app must be running for `link`/`unlink`/
`ignore`/`unignore` (reads still work without it). An open Graph view refreshes
live because the app updates its own state directly; the filesystem watcher on
`topology.json` remains as a fallback for a hand edit or any other writer. See
the [Graph](./graph.md) guide for the visual surface.

## Commands

```bash
wardian agent
wardian agent <name-or-uuid>
wardian agent show [name-or-uuid]
wardian agent list
wardian agent list --scope all
wardian agent restart <name-or-uuid>
wardian agent rename <name-or-uuid> <new-name>
wardian agent delete <name-or-uuid> --confirm <current-agent-name>
wardian agent delete <name-or-uuid> --confirm <current-agent-name> --force
wardian agent pause <name-or-uuid>
wardian agent resume <name-or-uuid>
wardian agent models --provider codex --refresh
wardian agent spawn --provider codex --class Reviewer --name reviewer-a1 --workspace <absolute-workspace-path>
wardian agent spawn --provider codex --class Reviewer --name reviewer-a1 --workspace <absolute-workspace-path> --model <model-id> --reasoning-effort <effort>
wardian agent update <name-or-uuid> --class Reviewer
wardian agent update <name-or-uuid> --workspace <absolute-workspace-path>
wardian agent update <name-or-uuid> --description "Owns frontend release follow-up"
wardian agent update <name-or-uuid> --description "" # clear the memo
wardian agent update <name-or-uuid> --model <model-id> --reasoning-effort <effort>
wardian agent clone <name-or-uuid> --name coder-a2
wardian agent worktree list
wardian agent worktree enable <name-or-uuid> --name review-fixes
wardian agent worktree join <name-or-uuid> --worktree <absolute-worktree-path-or-id>
wardian agent worktree disable <name-or-uuid>
wardian agent wait reviewer-a1 --until idle --timeout 10m
wardian agent wait reviewer-a1 --until idle --next --timeout 10m
wardian agent watch reviewer-a1 --until output:REVIEW_DONE --include transcript,output,delivery --timeout 10m
wardian agent watch reviewer-a1 --include raw_output --raw
wardian team list
wardian team show <team-name-or-id>
wardian team create <name> --agent <name-or-uuid> [--agent <name-or-uuid>...]
wardian team rename <team-name-or-id> <new-name>
wardian team add <team-name-or-id> <agent-name-or-uuid> [...]
wardian team remove <team-name-or-id> <agent-name-or-uuid> [...]
wardian team split <team-name-or-id> --name <new-team-name> --agent <name-or-uuid> [...]
wardian team delete <team-name-or-id>
wardian watchlist list
wardian watchlist show <watchlist-name-or-id>
wardian watchlist create <name>
wardian watchlist rename <watchlist-name-or-id> <new-name>
wardian watchlist add-team <watchlist-name-or-id> <team-name-or-id>
wardian watchlist remove-team <watchlist-name-or-id> <team-name-or-id>
wardian watchlist add-agent <watchlist-name-or-id> <agent-name-or-uuid>
wardian watchlist remove-agent <watchlist-name-or-id> <agent-name-or-uuid>
wardian watchlist delete <watchlist-name-or-id>
wardian inbox list [--type <type,...>] [--source <source,...>] [--unread] [--limit <n>] [--offset <n>]
wardian automation node-types
wardian automation list
wardian automation validate <path-to-automation.md>
wardian automation exec <path-to-library-automation.md> --provider codex --workspace <absolute-workspace-path>
wardian automation runs
wardian automation run-show <blueprint-id> <run-id>
wardian automation replay <blueprint-id> <run-id>
wardian automation schedule add --blueprint <id> --name <name> \
wardian automation schedule add --blueprint <id> --name <name> \
  --workspace <absolute-workspace-path> --every 60
wardian automation schedule update <schedule-id> \
  --workspace <absolute-workspace-path> --daily 09:30
wardian automation schedule add --blueprint <id> --name <name> \
  --workspace <absolute-workspace-path> \
  --weekly Mon,Wed,Fri@09:30 --repeat-every 2
wardian automation schedule list
wardian library list [skills|prompts|classes|automations|mcps] [--flat]
wardian library show <section/path> [--content]
wardian library read <section/path>
wardian library create <section/path> --stdin
wardian library write <section/path> --file <path>
wardian library move <section/path> <section/new-path>
wardian library delete <section/path>
wardian library star <section/path>
wardian library unstar <section/path>
wardian library tags <section/path> --set <tag> [--set <tag>...]
wardian library deploy <skills/path> --targets user:global,class:Reviewer
wardian library deploy <skills/path> --clear
wardian library deployments <skills/path>
wardian library orphans
wardian library orphan delete --target class:Reviewer --skill old-planner
wardian library restore-default classes/Reviewer
wardian telemetry summary
wardian telemetry summary --horizon month --dimension model
wardian conversation list
wardian conversation list --agent <agent-id-or-name>
wardian conversation list --scope all
wardian conversation show <conversation-id>
wardian ask reviewer-a1 --stdin --timeout 10m
wardian ask reviewer-a1 "review this" --targets reviewer-a2,reviewer-a3 --timeout 10m
wardian reply ask_0123456789abcdef --status done --stdin
wardian send "review this" --to coder-a1
wardian send --as-command "/goal test" --to coder-a1
wardian send "review this" --to reviewer-a1 --wait-until idle --timeout 10m
wardian send "status?" --to class:Coder
wardian send "stand down" --to all
wardian notify update "The migration is ready for review" --title "Inbox refactor"
wardian notify approval "Production deployment is prepared" --title "Deploy production" --action "Run the production deployment" --risk "This changes live traffic" --choice "Deploy" --choice "Do not deploy" --wait
```

Agent creation validates `--class` against the classes in the active Wardian
home and validates `--provider` against the supported provider IDs before
starting a provider or reserving an agent name. Both values are
case-insensitive and are stored canonically (`Reviewer`, `codex`). Unknown
classes and providers fail without creating agent state. Names and workspace
paths are also validated by the creation command.

Model IDs and reasoning efforts are provider-discovered; use
`wardian agent models --provider <provider>` to inspect the currently exposed
catalog rather than relying on a fixed CLI list. Provider-specific live
selection may reject a model/effort pair that is not present in that catalog.

`agent restart` restarts the provider while preserving the Wardian agent, its
habitat, and saved session history. Use it after `agent update` when the update
reports `restart_required`, including class changes. `agent delete` is
permanent: it removes the agent, its habitat, and its session history, while
leaving the project workspace files untouched. It always requires the exact
current agent name as `--confirm <current-agent-name>`. Without `--force`, it
refuses to delete while the provider process is running; with `--force`, it
explicitly terminates that provider first. Rename is live and does not restart
the provider. The new name resolves immediately
for `send` and `ask`. Agent-owned Wardian history is cascaded with deletion;
project workspace files are never removed.

`send` is one-way: it reports delivery evidence or queueing, but does not
return the target's answer. Use `ask` when an automation step needs a durable
structured reply. When a normal live message is queued because the target is
busy, Wardian persists it until the target reaches a later idle or ready
observation; there is no timer-based retry or age expiry. Pending mailbox work
survives an app restart and gets a status-gated delivery opportunity after the
agent is restored. A native live message becomes `provider_accepted` only after
the provider starts the submitted turn. If terminal state becomes ambiguous
after input is written, Wardian marks the delivery failed instead of replaying
the message. For a live message, `send --wait-until idle` waits for the
provider-confirmed completion of the specific delivered turn rather than
treating any brief Idle status observation as completion.

## Common Automations

Inspect your neighbors (default):

```bash
wardian agent list --fields name,class,provider,workspace,status
```

Inspect the full roster when coordinating across multiple neighbor sets:

```bash
wardian agent list --scope all --fields name,class,provider,workspace,status,status_source
```

Hand a bounded review task to a peer and wait for response evidence:

```bash
wardian ask reviewer-a1 --file review-prompt.md --timeout 10m
```

Ask several named peers for individually accountable replies. The initial target
and each comma-separated `--targets` value are explicit names or UUIDs; `all`
and `class:<ClassName>` are not accepted:

```bash
wardian ask reviewer-a1 --file review-prompt.md --targets reviewer-a2,reviewer-a3 --timeout 10m
```

Answer a structured ask from inside the target agent session:

```bash
cat <<'EOF' | wardian reply ask_0123456789abcdef --status done --stdin
Reviewed the patch. No blocking findings.
EOF
```

Send a prompt to an existing agent and wait for provider-confirmed completion
of that delivered turn:

```bash
wardian send --file prompt.md --to coder-a1 --wait-until idle --timeout 10m
```

Watch retained readable output for a deterministic marker:

```bash
wardian agent watch coder-a1 --until output:READY_FOR_REVIEW --include transcript,output,delivery --timeout 10m
```

A conditional `agent watch --until ...` starts at the cursor observed when the
command begins, so retained status history cannot satisfy a new wait. Omit
`--until` to inspect retained history, or pass `--since <cursor>` when a
historical condition is intentional.

Send a human-facing update only when it changes the user's understanding or next decision:

```bash
wardian notify update "The release build passed; one migration risk remains" \
  --title "Release readiness"
```

Request approval only for an irreversible, external, security-sensitive, or materially costly action (or when the user explicitly requested it):

```bash
wardian notify approval "Deployment is ready" \
  --title "Deploy production" \
  --action "Run the production deployment" \
  --risk "This changes live traffic and may require rollback" \
  --choice "Deploy" \
  --choice "Do not deploy" \
  --expires-in 30m \
  --wait
```

`notify` requires a managed agent session. Updates create a durable Inbox record. Approval requires an action, risk, two to five explicit choices, and an expiry; only one unresolved manual approval may exist per agent. `--wait` returns the structured decision or `expired`; expiry never permits the action automatically. Provider-native permission prompts remain separate provider action-needed events.

Inspect provider-adapted transcript text, sanitized terminal fallback, or raw PTY evidence:

```bash
wardian agent watch Librarian --include transcript
wardian agent watch Librarian --include output
wardian agent watch Librarian --include raw_output --raw
```

PowerShell:

```powershell
wardian agent watch Librarian --include transcript
wardian agent watch Librarian --include output
wardian agent watch Librarian --include raw_output --raw
```

Run a saved automation through the app-owned backend:

```bash
wardian automation list
wardian automation validate <absolute-workspace-path>/library/automations/autoreview.md
wardian automation exec <absolute-workspace-path>/library/automations/autoreview.md \
  --provider codex \
  --workspace <absolute-workspace-path> \
  --input '{"target":"HEAD"}' \
  --bind reviewer=codex
wardian automation runs
wardian automation run-show autoreview <run-id>
```

PowerShell:

```powershell
wardian automation list
wardian automation validate <absolute-workspace-path>\library\automations\autoreview.md
wardian automation exec <absolute-workspace-path>\library\automations\autoreview.md `
  --provider codex `
  --workspace <absolute-workspace-path> `
  --input '{"target":"HEAD"}' `
  --bind reviewer=codex
wardian automation runs
wardian automation run-show autoreview <run-id>
```

By default, `automation exec` is a live-control command: it requires the desktop app to be running for the same `WARDIAN_HOME`, routes execution through app-owned runtime state, and accepts automation files under `<wardian-home>/library/automations`. The `mock` executor is reserved for automation-engine fixture tests and should not be used as a normal CLI launch path.

Use `automation list` to discover Library blueprints before running them. Its JSON
rows include the parsed `blueprint_id`, display `name`, `entry_ref`, and absolute
`automation_path`; the id comes from blueprint frontmatter rather than the
filename. Use `automation list --pretty` for one human-readable row per entry.
Malformed blueprints remain in the listing with an `error` field. Use
`automation runs`, `automation run-show <blueprint-id> <run-id>`, and
`automation replay <blueprint-id> <run-id>` to inspect durable run artifacts under
`<wardian-home>/logs/automations`.

Author and deploy Library assets from an agent terminal:

```bash
cat <<'EOF' | wardian library create prompts/review.md --stdin
# Review

Review the current patch and return findings first.
EOF
wardian library star prompts/review.md
wardian library tags prompts/review.md --set review --set daily
wardian library list --flat
wardian library deploy skills/review/planner --targets user:global,class:Reviewer
wardian library deployments skills/review/planner
wardian library deploy skills/review/planner --clear
wardian library read classes/Reviewer
```

PowerShell:

```powershell
@"
# Review

Review the current patch and return findings first.
"@ | wardian library create prompts/review.md --stdin
wardian library star prompts/review.md
wardian library tags prompts/review.md --set review --set daily
wardian library list --flat
wardian library deploy skills/review/planner --targets user:global,class:Reviewer
wardian library deployments skills/review/planner
wardian library deploy skills/review/planner --clear
wardian library read classes/Reviewer
```

`wardian library` is a disk-backed authoring surface for reusable assets. It can list, show, read, create, edit, move, delete, tag, star, and deploy Library entries without the desktop app running. `list --flat` emits entry rows only, including when no section is supplied. Prompt and automation refs must end in `.md`, and skills cannot contain other skills. `deploy --targets` requires existing targets and deduplicates repeated refs; use explicit `deploy --clear` to remove the final target safely. Class definitions and instruction files initialize on first class access. Automation entries under `library/automations` are blueprint files only: use `wardian automation list` for discovery and the other `wardian automation` verbs for automation-specific behavior.

Use `conversation list` and `conversation show <conversation-id>` to inspect durable agent-owned conversation archives. Inside a Wardian-managed agent terminal, `conversation list` defaults to that agent through `WARDIAN_SESSION_ID`. Outside a managed agent terminal, pass `--agent <agent-id-or-name>` or `--scope all`. `show` returns the manifest and agent-readable `conversation.jsonl` narrative, not provider-private raw logs. Wardian refreshes `turns.jsonl` whenever it refreshes the normalized archive, including open conversations, so readers can use `manifest.json` plus `turns.jsonl` as the cheap per-request index and fall back to `conversation.jsonl` only for full detail. A `turns.jsonl` row means one user-originated request plus following assistant, tool, and lifecycle records until the next user-originated request or boundary; provider tool-call IDs do not create separate turn rows. Context rows such as AGENTS.md injections, goal continuations, and lifecycle-only records are typed in `request.kind` so agents can skip them when building summaries. Agents and external tools should use this CLI surface or bounded reads of `agents/<agent-id>/conversations/index.jsonl`; do not recursively crawl under `agents/*`, because agent directories can contain worktrees, provider caches, screenshots, and dependencies. Direct readers must treat `index.jsonl` as append-only upsert history and keep the latest row per `conversation_id`.

Mutating commands use Wardian's local control endpoint and require the desktop app to be running for the same `WARDIAN_HOME`. This includes agent lifecycle commands, agent worktree commands, live `automation exec`, and `send`.

`automation list`, `automation validate`, `automation parse`, `automation normalize`, `automation node-types`, `automation runs`, `automation run-show`, `automation replay`, `library`, `conversation list`, `conversation show`, `inbox list`, `team`, and `watchlist` can run from disk without the desktop app.

`agent spawn` requires both `--provider` and `--class` so the created agent's runtime and role are explicit.

`agent models --provider <provider>` returns the installed provider's current
model catalogue and compatible effort levels. Use `--refresh` after a provider
or account change. Keep the provider default for routine bounded tasks; for
complex, ambiguous, multi-step work, choose only a listed model/effort pair.
Do not guess provider IDs or use high effort solely because an agent's class
sounds senior.

`agent update <target>` changes an existing agent through the running app. Use
`--class <ClassName>` to assign an existing class and regenerate the agent's
instruction include directories. Use `--workspace <absolute-path>` when an
ordinary agent's workspace folder was moved or renamed; the destination must
already exist. Both flags can be supplied together and are committed to live
and persisted state as one update. The JSON response reports `updated_fields`
and `restart_required`. Wardian does not interrupt a running provider process,
so restart the agent when `restart_required` is true before relying on the new
class instructions, working directory, model, or reasoning effort. Pass
`--model ""` or `--reasoning-effort ""` to return to the provider default.
Managed worktree agents must use
`agent worktree join` or `agent worktree disable` instead.

`agent worktree list` returns the worktrees currently managed by Wardian with source folder, worktree folder, display name, and member agent IDs. `agent worktree enable`, `join`, and `disable` are live-control commands. They reuse the same backend logic as the Source Control panel and force a fresh agent session after changing the runtime workspace. `disable` removes the assignment only; it does not delete the physical worktree folder.

`team` and `watchlist` read and write `<wardian-home>/watchlists/index.json`. Read commands accept the current v2 shape with global teams and legacy flat watchlist arrays, then return `schema: 1` JSON for automation. Mutation commands write canonical v2 JSON with camelCase storage keys, resolve agent names or UUIDs through the same roster state as `agent list`, and update `topology.json` when team creation, add, or split operations seed new team clique edges. If the desktop app is running for the same `WARDIAN_HOME`, the CLI sends a best-effort reload notification so the roster picks up the change. `send --to team:<name>` is still not implemented.

Team mutation validation rejects duplicate team names, unknown agents, ambiguous names, and operations that would leave a team empty. Deleting a team removes dangling team entries from watchlists. Removing or splitting team members does not remove existing topology edges; the communication graph remains user-owned after a team has seeded edges.

`agent wait <target> --until <status>` blocks inside the CLI process until a single agent name or UUID reaches a normalized status such as `idle`, `processing`, `action_required`, `off`, or `error`. Plain `wait` returns immediately when the target is already in the requested status. Add `--next` to wait for a newer matching observation. Use `--timeout` with `ms`, `s`, or `m` units.

`agent watch <target>` returns a live snapshot with agent status, a provider-adapted `transcript`, sanitized retained terminal `output`, delivery details, and a cursor. Raw PTY text is not returned by default. Add `--raw` or `--include raw_output` only when debugging terminal rendering, ANSI/control sequences, or PTY transport behavior. `raw_output.text` may contain escape sequences and prompt repaint fragments.

`transcript` is extracted from structured provider lines when Wardian has a provider adapter. This slice covers Codex, Claude, Gemini, Antigravity, OpenCode, Pi, and the mock provider. Gemini can backfill completed assistant text from Gemini chat logs, Antigravity can backfill completed assistant text from its conversation transcript, OpenCode can backfill assistant text from its session database, and Pi can backfill completed assistant text from its session JSONL when the live TUI does not expose a clean structured line. Ambiguous provider lines fall back to sanitized terminal `output` until provider-specific transcript adapters are added. `output` is the compatibility surface for `--until output:<substring>` and is cleaned of common ANSI, OSC, cursor, and clear-line controls. Internally, marker matching also checks transcript text and the raw PTY tap so existing token-based automation keeps working without returning raw text by default.

Add `--until` to block until `status:<status>`, `output:<substring>`, `event:<kind>`, or `delivery:<state>` is observed. `watch` accepts only one name or UUID in this slice. `--follow` is reserved and returns `not_supported`.

`ask <target>` sends one prompt to one Wardian-managed agent and creates a durable task interaction with a backend-owned `request_id`. When the target is off, normal message delivery uses that agent's headless provider transport; the target is shown as `Headless` while its agent-level lease is active, whether the provider turn resumes an existing session or starts fresh. Wardian appends reply instructions to the delivered prompt and waits for the target to execute `wardian reply <request-id> --status done --stdin`. The structured ask path completes only when the task interaction receives an explicit reply interaction. Echoed request IDs, terminal repaint text, and output markers do not complete the ask.

Add comma-separated `--targets <name-or-uuid,...>` to fan the same structured request out to several explicitly named peers. Wardian appends reply instructions to each delivered prompt and waits for every target to execute `wardian reply <request-id> --status done --stdin`. The structured ask path completes only when each task interaction receives an explicit reply interaction. Echoed request IDs, terminal repaint text, and output markers do not complete an ask.

Single-target JSON responses include `request_id`, `reply.status`, `reply.body`, delivery evidence, watch events, and retained output. Multi-target responses contain `targets[]`, with a separate `request_id`, delivery evidence, reply/watch evidence, and outcome for each target. Outcomes are `completed`, `timed_out`, `delivery_failed`, or `cancelled`. Wardian delivers all multi-target requests before waiting; the shared timeout closes outstanding interactions with a failed reply, and cancelled requests are closed the same way, so late replies are rejected. `reply.status` can be `done`, `blocked`, or `failed`. If a target runtime is booting, busy, action-required, or missing a safe input channel, Wardian keeps the interaction queued and reports the delivery state instead of relying on a fixed sleep before terminal injection.

Use `--until output:<token>` only when you explicitly need the older output-substring mode, such as manual provider output matching or compatibility with agents that cannot run `wardian reply`. Output markers are weaker evidence than structured replies because they are derived from transcript or terminal output. Other explicit watch conditions such as `status:<status>`, `event:<kind>`, and `delivery:<state>` also preserve the watch-based behavior. Multi-target asks require the default `--until reply` mode. `ask` rejects `all`, `class:<ClassName>`, and reserved `--thread` usage with `not_supported`.

`reply <request-id> --status done|blocked|failed --stdin` records a structured reply through the live control endpoint. Wardian resolves the request ID against the interaction store. Unknown request IDs fail deterministically, and duplicate replies are rejected unless a future explicit idempotency policy says otherwise. When run from a Wardian-managed agent terminal, `WARDIAN_SESSION_ID` is used to verify that the reply came from the target agent for that request. Replies submitted outside a Wardian-managed session are accepted for this first live-control slice so a human terminal can unblock a request, but that caller identity is not authenticated.

`send` submits a provider-aware message into the target agent runtime. Targets can be an agent name, UUID, `class:<ClassName>`, or `all`. By default:
- `--to all` broadcasts within your **neighbors**, not globally.
- `--to class:ClassName` resolves within your neighbors.
- Bare agent names resolve neighbors-first; explicit UUIDs always work globally.

For an ordinary `send`, the default `queue-if-busy` policy uses a live provider surface when it is safe. If the target is off or errored, Wardian instead runs the target agent headlessly. The target receives an agent-level lease for that turn, which makes it appear purple as `Headless`; a saved provider session is resumed when one exists, while fresh runs do not invent one. The response is retained in `wardian agent watch` and the conversation archive. `--timeout` bounds a headless delivery (up to 15 minutes); timeout or cancellation stops the provider's full process tree before its lease is released. If another sender acquires the lease first, the message is queued rather than retried against the provider. Resume, clear, pause, and remove take the same durable lease before their local lifecycle gate; if an active headless turn owns it, the lifecycle action stops before changing the agent. Use `--queue-policy mailbox-only` when the message must wait for a later interactive turn. `--as-command` stays mailbox-delivered while an agent is off because a provider slash command requires an interactive surface.

Use `--scope all` to broadcast/resolve globally (orchestration across multiple neighbor sets only). `--stdin` reads the message from standard input, and `--file <path>` reads it from a file. By default, Wardian keeps inter-agent attribution and delivers messages with a `From <sender>:` prefix when sender context is available. Use `--as-command` for provider slash commands that must start at the first input token:

```bash
wardian send --as-command "/goal test" --to coder-a1
printf '%s' '/status' | wardian send --stdin --as-command --to coder-a1
```

PowerShell:

```powershell
"/status" | wardian send --stdin --as-command --to coder-a1
```

`--as-command` sends the exact message body without the attribution prefix while still using the normal provider-aware submit path. It accepts only one explicit agent name or UUID, rejects `all` and `class:<ClassName>` with `not_supported`, and cannot be combined with `--thread`.

`--wait-until <status>` is available for single-agent targets. A normal live send first waits for its own `submit_started` delivery boundary; `--wait-until idle` then waits for that exact provider turn's `turn_completed` event, rather than a retained or transient Idle observation. For an offline headless turn, `--wait-until idle` waits for that turn's durable `provider_applied` delivery event instead: the returned agent snapshot correctly remains `off` rather than inventing a live Idle session. `--thread` is reserved but not implemented yet; when the app is running, using it returns `not_supported`.

Successful `send` responses include `input_mode` and `delivery[]`; command sends also include `delivery[].input_mode` so automation can confirm command delivery. Failed or partial delivery returns a nonzero exit with JSON on stderr and `details.delivery[]`, including `runtime_state`, `delivery_state`, and provider-specific input errors.

List filters:

- `--status <status>` filters by normalized status, such as `idle`, `processing`, or `action_required`.
- `--class <class>` filters by agent class.
- `--workspace <absolute-path>` filters by workspace and implies `--scope all`.

Output options:

- `--fields name,status,uuid` returns indented JSON with only those fields.
- `--field status` returns one bare value plus a newline.
- `--field status_source` returns `live` or `persisted`.
- `--verbose` adds `pid`, `started_at`, `last_status_at`, and `visibility` (why each neighbor is visible: `manual` or `rule:workspace-fallback`).
- `--pretty` returns aligned text for interactive inspection instead of JSON.

Default JSON is indented for terminal readability. It includes `schema: 1` and an `agent` or `agents` payload with `name`, `uuid`, `class`, `provider`, `workspace`, and `status`.

## Presenting artifacts

Agents running inside a Wardian-managed terminal can present any file under
their primary workspace or additional granted directories:

```bash
wardian artifact present ./report.md --title "Report for review"
```

PowerShell:

```powershell
wardian artifact present .\report.md --title "Report for review"
```

The desktop app must be running, and `WARDIAN_SESSION_ID` identifies the origin
agent. Re-presenting the same canonical path normally appends a version to the
active thread; use `--new` for a distinct thread or `--artifact <id>` to require
an exact existing thread. `--address <comment-id>` can be repeated. The command
returns the durable artifact, version, and presentation IDs as JSON only after
the Workbench accepts the background tab transaction. A UI delivery failure
reports the already-persisted artifact details and does not delete the version.

Use `wardian artifact show <artifact-id> [--version <version-id>]` to inspect
durable metadata. Show can fall back to the on-disk artifact store when the app
is not running; present never does, because authorization and UI delivery are
live runtime contracts.

## Important Limits

- The desktop app must be running for live-control commands such as `send`, `spawn`, `pause`, `resume`, `kill`, and default `automation exec`.
- `WARDIAN_HOME` must match between the app and CLI when you expect shared live state.
- Team and watchlist mutation commands write disk state directly and best-effort notify the running app. `send --to team:<name>` is not implemented yet.
- Raw terminal output can include escape sequences; prefer transcript or sanitized output unless debugging PTY behavior.

## Exit Codes

| Code | Meaning |
|---:|---|
| 0 | Success |
| 1 | Generic command error |
| 2 | Agent not found |
| 3 | `WARDIAN_SESSION_ID` is not set for self lookup |
| 4 | Wardian state database is unavailable |
| 5 | Lookup matched multiple agents |
| 6 | Desktop app is not running for a live control command |

Errors are written to stderr as JSON:

```json
{
  "schema": 1,
  "error": {
    "code": "not_in_session",
    "message": "WARDIAN_SESSION_ID environment variable is not set",
    "hint": "Pass a name or uuid to look up a specific agent from outside a Wardian-managed agent process: `wardian agent <name>`.",
    "details": {
      "command": "agent",
      "requested": "self"
    }
  }
}
```

## Related Links

- [Getting Started](./getting-started.md)
- [Watchlists](./watchlists.md)
- [Command Panel](./command-panel.md)
- [Automations](../automations/index.md)
- [Native E2E Harness](../developer/native-e2e.md)

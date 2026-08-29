# Agent development environment feature matrix

Human-readable companion to the [backing CSV](./agent-development-environment-feature-matrix.csv).

- Reviewed: 2026-08-29
- Coverage: 173 feature rows across 15 areas
- Products: Wardian, Orca ADE, cmux, Conductor, and T3 Code
- Status labels: `[YES]`, `[PARTIAL]`, `[EXPERIMENTAL]`, `[NO]`, and `[NOT EVIDENCED]`
- `[NOT EVIDENCED]` means the capability was not found in the reviewed first-party material; it is not proof of absence.

## Scope and evidence corpus

The four-product comparison cohort (Orca ADE, cmux, Conductor, and T3 Code) was
named by the user; Wardian is included as the first-party baseline. The cohort
was not sampled from the market. The selection criterion was verifiability:
these are the open-source ADEs that can be checked against first-party source.
Cursor and Devin are serious competitors, but they are excluded because they
cannot be verified on that same source basis. This file is therefore not a
survey of the field; a Cursor/Devin comparison would belong in a separately
scoped, claims-based tier.

The evidence corpus differs by product. The original Wardian assessment was
documentation-first; this correction pass adds repository source and metadata.
Orca ADE is assessed against its product documentation and public repository
pages; cmux against its product site/documentation and public repository pages;
Conductor against its public product documentation; and T3 Code against its
documentation and public repository pages. The source-inclusive Wardian recheck
moved four of the 19 previously `NOT EVIDENCED` rows: License and openness and
Agent Client Protocol provider transport moved to `YES`, while Custom agent registration
and fork/resume templates and AI versus human line attribution moved to
`PARTIAL`; 15 remain `NOT EVIDENCED` at the claimed scope.

## Count analytics

### Feature type coverage

Here, feature type means the feature area used in the CSV.

| Feature type | Feature count |
| --- | ---: |
| Agents | 35 |
| Collaboration | 1 |
| Communication | 16 |
| Coverage | 1 |
| Extensibility | 11 |
| Git | 10 |
| Notifications | 3 |
| Observability | 4 |
| Projects | 17 |
| Remote | 17 |
| Scope | 10 |
| Security | 6 |
| Settings | 6 |
| Surfaces | 25 |
| Workflows | 11 |

### Assessment counts by ADE

Counts are calculated across all 173 feature rows. The three `Description-only` entries are product-level rows that describe the product rather than assess a specific capability.

| ADE | YES | PARTIAL | EXPERIMENTAL | NO | NOT EVIDENCED | Description-only |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Wardian | 100 | 55 | 0 | 0 | 15 | 3 |
| Orca ADE | 93 | 63 | 0 | 0 | 14 | 3 |
| cmux | 60 | 59 | 3 | 3 | 45 | 3 |
| Conductor | 55 | 90 | 0 | 2 | 23 | 3 |
| T3 Code | 90 | 62 | 0 | 0 | 18 | 3 |

Read each `YES` count alongside that product's `PARTIAL`, `NO`, and `NOT
EVIDENCED` counts. Evidence coverage is uneven: cmux carries 45 `NOT
EVIDENCED` rows, 26% of the matrix. This is a scoped first-party evidence
matrix with uneven coverage, not a leaderboard.

## Coverage by area

| Feature area | Features |
| --- | ---: |
| Agents | 35 |
| Collaboration | 1 |
| Communication | 16 |
| Coverage | 1 |
| Extensibility | 11 |
| Git | 10 |
| Notifications | 3 |
| Observability | 4 |
| Projects | 17 |
| Remote | 17 |
| Scope | 10 |
| Security | 6 |
| Settings | 6 |
| Surfaces | 25 |
| Workflows | 11 |

## Detailed comparison

### Agents

#### Supported first-class providers

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - six provider CLIs: Antigravity, Claude, Codex, OpenCode, Pi, and Gemini CLI; Gemini is unmaintained and Antigravity is preferred |
| Orca ADE | [YES] - Claude Code, Codex, Grok, Cursor, GitHub Copilot, OpenCode, Pi, and many more |
| cmux | [PARTIAL] - supports any CLI; Claude Code Teams and oh-my-opencode are called out |
| Conductor | [YES] - Claude Code, Codex, Cursor, and OpenCode first-party |
| T3 Code | [YES] - Codex, Claude, Cursor, Grok Build, and OpenCode |

**Comparison note:** Provider lists change; this row reflects current first-party docs/repositories reviewed on the comparison date.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/README.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 3](https://github.com/stablyai/orca), [source 4](https://cmux.com/), [source 5](https://www.conductor.build/docs), [source 6](https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md)

#### Any arbitrary CLI agent

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - custom arguments and terminal support, but native orchestration is provider-aware |
| Orca ADE | [YES] - explicit any CLI support |
| cmux | [YES] - core terminal premise is any CLI |
| Conductor | [PARTIAL] - terminal can run commands; agent integrations are curated |
| T3 Code | [YES] - provider CLIs are external and configurable; arbitrary CLI terminal use is available |

**Comparison note:** Orca and cmux make arbitrary CLI support explicit; Wardian/T3 add structured provider integrations.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/agents-overview.md), [source 2](https://github.com/stablyai/orca), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/reference/big-terminal-mode), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md)

#### Provider binary path and launch options

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider-specific launch options, custom arguments, headless flags, and shell/session policy |
| Orca ADE | [PARTIAL] - provider guides and supported agents; per-provider launch controls less fully evidenced |
| cmux | [PARTIAL] - shell, config, custom commands, and terminal environment |
| Conductor | [PARTIAL] - agent behavior, project instructions, and run modes; binary configuration less central |
| T3 Code | [YES] - binary discovery/path, provider cards, environment variables, launch options, and server-side provider config |

**Comparison note:** T3 and Wardian expose the most explicit provider launch configuration in their docs.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/agents-overview.md), [source 2](https://www.onorca.dev/docs/agents/supported), [source 3](https://github.com/manaflow-ai/cmux-skills), [source 4](https://www.conductor.build/docs/reference/agent-behavior), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md)

#### Provider readiness and authentication status

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider readiness checks, auth status, supported-provider validation, and failure before launch |
| Orca ADE | [PARTIAL] - account switcher/usage and provider guides; readiness details not as explicit |
| cmux | [NOT EVIDENCED] in reviewed first-party sources beyond CLI/provider behavior |
| Conductor | [PARTIAL] - agent credentials and provider setup checks |
| T3 Code | [YES] - settings show authenticated/ready/missing provider state; auth happens on server |

**Comparison note:** This is separate from runtime Action Needed prompts.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/provider-readiness.md), [source 2](https://www.onorca.dev/docs/agents/usage-tracking), [source 3](https://www.conductor.build/docs/installation), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Models and reasoning controls

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider-discovered models and reasoning effort; selected provider controls differ |
| Orca ADE | [PARTIAL] - model/account usage is documented; universal model controls not evidenced |
| cmux | [NOT EVIDENCED] as a native model picker; terminal agent owns model choice |
| Conductor | [YES] - agent modes, reasoning/thinking, and provider-specific support matrix |
| T3 Code | [YES] - model pickers, OpenCode model catalogs, Codex/Claude configuration, Grok reasoning levels |

**Comparison note:** Wardian and T3 expose model state in their own UI; cmux generally leaves this to the CLI.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/agents-overview.md), [source 2](https://www.onorca.dev/docs/agents/usage-tracking), [source 3](https://www.conductor.build/docs/concepts/agent-modes), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-opencode.md)

#### Multiple provider accounts or profiles

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - provider instances/settings exist; account switching is provider-specific |
| Orca ADE | [YES] - account switcher and multi-account usage |
| cmux | [NOT EVIDENCED] as a cmux account-management feature |
| Conductor | [PARTIAL] - own subscriptions/API keys and provider setup; account switching not a central feature |
| T3 Code | [YES] - multi-account Codex/Claude provider instances, shadow homes, emails, endpoints, and secrets |

**Comparison note:** T3 has the most explicit multi-account workflows; Orca documents account switching and usage.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/provider-readiness.md), [source 2](https://www.onorca.dev/docs/agents/usage-tracking), [source 3](https://www.conductor.build/docs/cloud), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-codex.md), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-claude.md)

#### Spawn or launch an agent

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - GUI and CLI spawn with class/provider/name/workspace/model/reasoning |
| Orca ADE | [YES] - create worktree and launch agent/session |
| cmux | [PARTIAL] - create workspaces/panes and launch CLI commands |
| Conductor | [YES] - launch agents into new or shared workspaces |
| T3 Code | [YES] - new thread/task, background thread, and provider session start |

**Comparison note:** All support launch; Wardian and T3 explicitly expose agent/session spawn semantics.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/cli.md), [source 2](https://www.onorca.dev/docs/model/agents-sessions), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/first-workspace), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Parallel agents and fan-out

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - teams, workflows, broadcast, watchlists, and parallel agent sessions |
| Orca ADE | [YES] - fan one prompt across five isolated worktrees and compare/merge |
| cmux | [YES] - subagents/teammates become native panes/splits |
| Conductor | [YES] - multiple parallel workspaces or agents in one workspace |
| T3 Code | [PARTIAL] - background threads and multiple workspaces; orchestration is runtime-supported but less productized than Orca/Conductor |

**Comparison note:** Parallelism exists in all five, but the unit and control plane differ.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/concepts/parallel-agents), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Shared teams or native subagents

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Teams seed topology edges; agent classes and workflow assignments provide roles |
| Orca ADE | [PARTIAL] - nested subagents and agent teams are visible; team authoring model not fully documented |
| cmux | [YES] - Claude Code Teams and oh-my-opencode teammates in native splits |
| Conductor | [PARTIAL] - multiple agents may share a workspace; no native team graph documented |
| T3 Code | [PARTIAL] - threads may create child work; provider subagents are surfaced when reported |

**Comparison note:** cmux is strongest on provider-team panes; Wardian is strongest on durable topology/team relationship modeling.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/graph.md), [source 2](https://www.onorca.dev/docs/model/agents-sessions), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/concepts/parallel-agents), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-codex.md)

#### Agent roles classes or reusable setups

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Agent Classes, reusable Library entries, skills, prompts, and class instructions |
| Orca ADE | [PARTIAL] - skills registry/MCP and provider hooks; no Wardian-style class library evidenced |
| cmux | [PARTIAL] - skills and config/custom commands; no class abstraction |
| Conductor | [PARTIAL] - agent behavior/instruction files and reusable slash commands |
| T3 Code | [PARTIAL] - provider configuration, skills, commands, and project settings; no class registry evidenced |

**Comparison note:** Wardian's class/library model is a distinctive capability.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/library.md), [source 2](https://www.onorca.dev/docs/cli/skills), [source 3](https://github.com/manaflow-ai/cmux-skills), [source 4](https://www.conductor.build/docs/reference/slash-commands), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Autonomy modes and planning controls

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider launch modes, headless/background mode, workflow agent modes, and approval policy |
| Orca ADE | [PARTIAL] - full autonomy default with configurable launch behavior |
| cmux | [NOT EVIDENCED] as product-level autonomy modes; agent CLI owns policy |
| Conductor | [YES] - Plan Mode, Fast Mode, reasoning, goals, and provider-specific modes |
| T3 Code | [YES] - Supervised, Auto-accept edits, Auto, and Full access; per-thread |

**Comparison note:** T3 has the clearest user-facing four-mode permission model; Conductor exposes planning/fast agent modes.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/agents-overview.md), [source 2](https://www.onorca.dev/docs/model/agents-sessions), [source 3](https://www.conductor.build/docs/concepts/agent-modes), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/permission-modes.md)

#### Permission prompts and sandbox policy

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider approvals and manual Wardian approval requests are distinct and persisted |
| Orca ADE | [PARTIAL] - computer-use permission gate and provider permissions |
| cmux | [PARTIAL] - native terminal/CLI permissions; no unified sandbox policy evidenced |
| Conductor | [YES] - local permissions versus cloud sandbox and agent permissions |
| T3 Code | [YES] - supervised/auto/full access, provider-specific mapping, approval responses, and mobile handling |

**Comparison note:** Wardian's provider Action Needed versus workflow/manual approval distinction is modeled explicitly.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/cli/computer-use), [source 3](https://www.conductor.build/docs/reference/security-and-permissions), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/permission-modes.md)

#### Start and session lifecycle states

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Idle, Processing, Action Required, Error, Off plus provider evidence and terminal lifecycle |
| Orca ADE | [YES] - working, waiting, done, blocked, idle from OSC/hooks |
| cmux | [PARTIAL] - active/unread/attention and terminal process lifecycle; normalized agent states not central |
| Conductor | [PARTIAL] - workspace/agent states and checks; product lifecycle labels less uniform |
| T3 Code | [YES] - threads, turns, approvals, waiting input, completion, stopped, and provider session events |

**Comparison note:** Normalization depth differs: Wardian/T3 have explicit runtime projections; cmux exposes attention and terminal state.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/model/agents-sessions), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Pause stop resume restart

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - pause, resume, restart, clear, kill, clone, wait, and watch actions |
| Orca ADE | [YES] - restart chip, hibernation auto-resume, session controls |
| cmux | [PARTIAL] - close/reopen panes and restart commands; provider session lifecycle not unified |
| Conductor | [PARTIAL] - stop/restart/sleep/wake workspace/agent flows |
| T3 Code | [YES] - session stop, turn interrupt, server restart/reconnect, and thread lifecycle |

**Comparison note:** Orca and Wardian expose rich agent lifecycle actions; T3 has the most explicit orchestration command vocabulary.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/watchlists.md), [source 2](https://www.onorca.dev/docs/model/agents-sessions), [source 3](https://www.conductor.build/docs/cloud/working-with-cloud-workspaces), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Idle agent hibernation

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - session persistence and lifecycle controls; automatic hibernation not evidenced |
| Orca ADE | [YES] - pause idle/done background agent terminals and auto-resume |
| cmux | [YES] - opt-in routine Agent Hibernation kills idle background agent processes and resumes their saved sessions; critical-memory safety hibernation remains |
| Conductor | [PARTIAL] - cloud sleep/wake for environments; agent-specific hibernation not evidenced |
| T3 Code | [NOT EVIDENCED] as an automatic idle-agent hibernation feature |

**Comparison note:** Do not conflate cloud workspace sleep with agent-terminal hibernation.
**Sources:** [source 1](https://www.onorca.dev/docs/agents/hibernation), [source 2](https://github.com/manaflow-ai/cmux/blob/main/docs/agent-hooks.md), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/configuration.md), [source 4](https://www.conductor.build/docs/cloud/working-with-cloud-workspaces), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Session history and transcript discovery

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - saved history, conversation archive, telemetry, logs, and agent session inspection |
| Orca ADE | [YES] - scans on-disk transcripts; search by title/cwd/branch/model/preview and resume |
| cmux | [PARTIAL] - scrollback/session restore; transcript history search not a core feature |
| Conductor | [PARTIAL] - chats/transcripts shared in collaboration; archival search not richly documented |
| T3 Code | [YES] - thread sidebar/history, persisted event log, transcript projections, and environment scope |

**Comparison note:** Orca and T3 explicitly describe transcript discovery; Wardian emphasizes inspectable state and provider evidence.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/cli.md), [source 2](https://www.onorca.dev/docs/agents/session-history), [source 3](https://www.conductor.build/docs/cloud/collaboration), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Session restore after restart or crash

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - terminal/session state survives tab moves, pane changes, remote handoff, and app restart where provider permits |
| Orca ADE | [YES] - restores worktrees, tabs, splits, scrollback, focus, and running agent processes via daemon |
| cmux | [YES] - restores windows, workspaces, panes, cwd, scrollback, browser state, and agent sessions |
| Conductor | [PARTIAL] - cloud workspace persists after laptop closes; local UI restore details not as explicit |
| T3 Code | [YES] - durable server runtime, reconnect, client/server sync, and session persistence |

**Comparison note:** All have restore behavior, but Orca/cmux document UI restoration most explicitly.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workbench.md), [source 2](https://www.onorca.dev/docs/model/session-restore), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/cloud), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Native structured agent chat

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - normalized chat cards over provider events plus real terminal mode |
| Orca ADE | [YES] - experimental Chat UI over same PTY for supported providers |
| cmux | [NOT EVIDENCED] as a structured transcript composer; terminal is primary |
| Conductor | [PARTIAL] - shared chat/transcript and prompt together; not a general native chat across all agents |
| T3 Code | [YES] - message composer and provider runtime with structured assistant/tool/approval events |

**Comparison note:** Wardian and T3 both normalize provider events; Orca's chat is explicitly experimental and PTY-backed.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/grid.md), [source 2](https://www.onorca.dev/docs/agents/native-chat), [source 3](https://www.conductor.build/docs/cloud/collaboration), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Send follow-up steer or interrupt

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - CLI send/stdin/file, ask/reply, prompt injection, interrupt, and native delivery broker |
| Orca ADE | [YES] - steer agents, follow-ups, and prompt from mobile/Chat UI |
| cmux | [YES] - send input through CLI/socket and terminal panes |
| Conductor | [YES] - prompt together, chat, and agent session controls |
| T3 Code | [YES] - send turns, interrupt turns, provider approvals/input responses, and background thread launch |

**Comparison note:** This is a key overlap, but provider acknowledgement is not the same as completed work.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/agents/native-chat), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Attachments and file-aware prompts

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - chat composer file picker/drops/pastes and filesystem-backed artifacts |
| Orca ADE | [YES] - drag files/images to agents and editor autosave |
| cmux | [PARTIAL] - drag image upload over SSH and file open; agent prompt attachment model not central |
| Conductor | [YES] - attach files, notes, specs, screenshots, logs, and .context to workspace |
| T3 Code | [YES] - images and text/PDF/ZIP/other file attachments with server upload limits |

**Comparison note:** Attachment semantics vary from prompt context to workspace artifacts to terminal file transfer.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/grid.md), [source 2](https://github.com/stablyai/orca), [source 3](https://cmux.com/docs/ssh), [source 4](https://www.conductor.build/docs/first-workspace), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Prompt stash or draft persistence

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - prompt/library storage and conversation persistence; dedicated stash not evidenced |
| Orca ADE | [PARTIAL] - draft persistence in native Chat UI |
| cmux | [NOT EVIDENCED] in reviewed first-party sources |
| Conductor | [PARTIAL] - chat and transcript collaboration; dedicated prompt stash not evidenced |
| T3 Code | [YES] - prompt stash with attachments, restore, expiry, and retry upload |

**Comparison note:** T3's prompt stash is a distinctive documented feature.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/library.md), [source 2](https://www.onorca.dev/docs/agents/native-chat), [source 3](https://www.conductor.build/docs/cloud/collaboration), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Slash commands skills and prompt reuse

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Library prompts, skills, classes, provider-native skill deployment, and CLI access |
| Orca ADE | [YES] - slash commands, discovered skills, skills registry/MCP |
| cmux | [YES] - skills collection, custom commands, and agent skills |
| Conductor | [YES] - .claude/commands Markdown prompt files, skills, AGENTS.md/CLAUDE.md |
| T3 Code | [YES] - slash command menu, project/system/personal/app skills, and provider-native command deduplication |

**Comparison note:** This is one of the broadest overlaps; Wardian adds a durable Library and class model.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/library.md), [source 2](https://www.onorca.dev/docs/cli/skills), [source 3](https://github.com/manaflow-ai/cmux-skills), [source 4](https://www.conductor.build/docs/reference/slash-commands), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Provider feedback or diagnostic submission

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] as a general Wardian feature |
| Orca ADE | [NOT EVIDENCED] in reviewed Orca docs |
| cmux | [NOT EVIDENCED] in reviewed cmux docs |
| Conductor | [NOT EVIDENCED] in reviewed Conductor docs |
| T3 Code | [YES] - /feedback uploads T3/Codex thread and logs with shareable thread ID |

**Comparison note:** T3's provider feedback command is a narrow but explicit feature.
**Sources:** [source 1](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-codex.md)

#### Usage rate limits and cost tracking

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Analytics/telemetry matrix for agent/model/provider activity; cost support depends on provider telemetry |
| Orca ADE | [YES] - local usage/rate-limit windows, multi-account usage, and estimated cost |
| cmux | [NOT EVIDENCED] as provider usage accounting |
| Conductor | [PARTIAL] - own provider subscriptions/API keys; no product usage dashboard evidenced |
| T3 Code | [YES] - usage page, token/cost toggles, provider/model usage, and account/provider detail |

**Comparison note:** Orca and T3 explicitly expose usage/rate-limit views; Wardian's analytics is more event/telemetry-oriented.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/analytics.md), [source 2](https://www.onorca.dev/docs/agents/usage-tracking), [source 3](https://www.conductor.build/docs/cloud), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/usage.md)

#### Durable agent memory store and recall

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - SQLite-backed memory save/list/recall/update/remove with agent-scoped capabilities, revisions, audit evidence, chat projection, and workflow commits |
| Orca ADE | [PARTIAL] - provider hooks and memory files are surfaced; no equivalent product-owned durable memory store evidenced |
| cmux | [NOT EVIDENCED] in reviewed first-party sources |
| Conductor | [PARTIAL] - project context files and shared cloud workspace context; no equivalent agent-owned memory store evidenced |
| T3 Code | [NOT EVIDENCED] in reviewed first-party user/internals docs as a product-owned long-term memory store |

**Comparison note:** Provider session history is not the same feature as durable cross-session memory.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/agent-memory.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/developer/agent-memory.md), [source 3](https://www.onorca.dev/docs/agents/hooks-memory), [source 4](https://www.conductor.build/docs/first-workspace), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/README.md)

#### Evidence-first memory provenance and selective injection

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - raw evidence is retained and indexed, selected context is injected with provenance, and promoted knowledge remains traceable |
| Orca ADE | [NOT EVIDENCED] in reviewed first-party sources |
| cmux | [NOT EVIDENCED] in reviewed first-party sources |
| Conductor | [NOT EVIDENCED] in reviewed first-party sources |
| T3 Code | [NOT EVIDENCED] in reviewed first-party sources |

**Comparison note:** This is a distinctive Wardian design direction rather than a generic transcript/history feature.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/agent-memory.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/specs/2026-04-17-evidence-first-memory.md)

#### Durable agent identity and provider session IDs

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - durable roster identity, class/provider/workspace/worktree fields, exact provider session identity, and replacement-safe lifecycle |
| Orca ADE | [PARTIAL] - worktree/session records and transcript-based resume; identity model is less explicit |
| cmux | [YES] - stable surface/session mappings, hook session IDs, and persisted agent-session bindings |
| Conductor | [PARTIAL] - workspace/agent identity and cloud persistence; detailed provider-session identity not evidenced |
| T3 Code | [YES] - environment/project/thread/session identities and provider session state are server-owned |

**Comparison note:** Identity persistence is different from merely restoring a pane or terminal.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/specs/2026-04-20-agent-identity-and-status-tracking.md), [source 2](https://www.onorca.dev/docs/agents/session-history), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/agent-session-tracking-spec.md), [source 4](https://www.conductor.build/docs/concepts/workspaces-and-branches), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Fresh versus resume session policy

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - global and per-agent Start Fresh versus Resume Sessions policy with provider-specific semantics |
| Orca ADE | [YES] - session restore and resume same session ID; host reboot limitations are explicit |
| cmux | [YES] - automatic agent resume toggle, saved native resume command, and manual Resume fallback |
| Conductor | [PARTIAL] - workspace persistence/wake and session continuity; explicit fresh-versus-resume policy not richly documented |
| T3 Code | [PARTIAL] - thread/session continuation and reconnect; explicit user-facing fresh-versus-resume policy not evidenced |

**Comparison note:** Orca and cmux focus on restore; Wardian explicitly models the policy choice.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/settings.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/specs/2026-04-17-session-persistence-policy.md), [source 3](https://www.onorca.dev/docs/model/session-restore), [source 4](https://github.com/manaflow-ai/cmux/blob/main/docs/agent-hooks.md), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Provider-native skill discovery adaptation

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - projected skills and instruction roots adapt to Antigravity, Claude, Codex, OpenCode, Pi, and Gemini rather than copying into repositories |
| Orca ADE | [YES] - provider path guides, hooks, and skill-sharing paths adapt to agent CLI conventions |
| cmux | [YES] - hook installers and agent-specific extensions/config files support many CLIs |
| Conductor | [YES] - AGENTS.md/CLAUDE.md/.claude/commands and skills follow provider conventions |
| T3 Code | [YES] - system/personal/project/app skills and provider-specific command discovery |

**Comparison note:** All five support reusable agent context; Wardian and cmux document the most explicit multi-provider path adaptation.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/providers.md), [source 2](https://github.com/stablyai/orca/blob/main/docs/reference/agent-skill-provider-paths.md), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/agent-hooks.md), [source 4](https://www.conductor.build/docs/reference/agent-behavior), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Agent hook installation and lifecycle integration

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - provider hooks and runtime parsers are managed by Wardian; provider setup remains provider-aware |
| Orca ADE | [PARTIAL] - hooks/memory and managed status hooks |
| cmux | [YES] - cmux hooks setup/uninstall installs lifecycle, Feed, notification, and session-restore integrations for many agent CLIs |
| Conductor | [PARTIAL] - agent instruction files and setup/run scripts; generic hook installer not evidenced |
| T3 Code | [PARTIAL] - provider adapters/reactors and native runtime events; user-facing hook installer not evidenced |

**Comparison note:** cmux has the broadest explicit hook installer; Wardian/T3 own more of the provider runtime path.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/provider-runtimes.md), [source 2](https://www.onorca.dev/docs/agents/hooks-memory), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/agent-hooks.md), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Agent Client Protocol provider transport

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - OpenCode ACP provider transport (`opencode_acp`), including ACP JSON-RPC permission handling |
| Orca ADE | [NOT EVIDENCED] in reviewed first-party sources |
| cmux | [NOT EVIDENCED] as an ACP implementation; supports arbitrary CLIs through terminals |
| Conductor | [NOT EVIDENCED] in reviewed first-party sources |
| T3 Code | [YES] - ACP package/runtime and Cursor/Grok ACP provider support coexist with CLI drivers |

**Comparison note:** Wardian and T3 expose ACP as provider transport infrastructure, distinct from a general RPC client protocol.
**Wardian sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/src-tauri/src/delivery/native_session.rs), [source 2](https://github.com/wardian-app/Wardian/blob/main/src-tauri/src/delivery/native_broker.rs), [source 3](https://github.com/wardian-app/Wardian/blob/main/docs/developer/provider-runtimes.md)
**Sources:** [source 1](https://github.com/pingdotgg/t3code/tree/main/packages/effect-acp), [source 2](https://github.com/pingdotgg/t3code/tree/main/apps/server/src/provider/acp), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Claude context compaction

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] as a Wardian-specific automatic context compaction feature |
| Orca ADE | [PARTIAL] - provider session history and chat UI; compaction is provider-owned if available |
| cmux | [NOT EVIDENCED] as a cmux-owned context compaction feature |
| Conductor | [PARTIAL] - agent modes/context and provider behavior; automatic compaction not central |
| T3 Code | [YES] - Claude auto-compact setting, older-thread compact offer, and /compact command |

**Comparison note:** T3 exposes a provider-specific context-management setting instead of claiming universal compaction.
**Sources:** [source 1](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-claude.md)

#### External OpenCode server and model health

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - OpenCode provider runtime/configuration; external-server health matrix less explicit |
| Orca ADE | [YES] - OpenCode support and provider integration |
| cmux | [PARTIAL] - OpenCode hook/plugin and terminal integration |
| Conductor | [PARTIAL] - OpenCode first-party agent support; external server details not central |
| T3 Code | [YES] - local helper or external OpenCode server, password isolation, version check, model refresh/cache, unavailable-model preservation |

**Comparison note:** T3 has the most explicit external OpenCode server management in the reviewed docs.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/providers.md), [source 2](https://www.onorca.dev/docs/agents/supported), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/agent-hooks.md), [source 4](https://www.conductor.build/docs), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-opencode.md)

#### Thread pin settle and pull-request linking

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - Inbox/workflow/agent organization; no equivalent thread sidebar settlement contract |
| Orca ADE | [PARTIAL] - pin/archive/filter worktrees and activity; thread PR linking not evidenced |
| cmux | [PARTIAL] - workspace pinning/groups and PR metadata; thread settlement not evidenced |
| Conductor | [PARTIAL] - workspace/PR lifecycle and collaboration; thread pin/settle not evidenced |
| T3 Code | [YES] - pinned threads, settled state, PR-link association, auto-settle on merged PR, ordering, and cross-environment visibility |

**Comparison note:** T3's thread sidebar is a distinct organization unit from Orca worktrees and cmux workspaces.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://www.conductor.build/docs/reference/checks), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Conversation-derived thread title generation

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - contextual agent opens/descriptions and completion summaries; dedicated title generation not central |
| Orca ADE | [PARTIAL] - worktree/session titles and repo context |
| cmux | [YES] - opt-in workspace/tab auto-naming from recent agent conversation |
| Conductor | [PARTIAL] - workspace/thread names and context; exact summarizer behavior not evidenced |
| T3 Code | [YES] - generated/regenerated thread titles with in-progress state and manual-name precedence |

**Comparison note:** cmux and T3 both summarize agent conversation for navigation labels.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/agents-overview.md), [source 2](https://github.com/manaflow-ai/cmux/blob/main/docs/workspace-auto-naming.md), [source 3](https://www.conductor.build/docs), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

### Collaboration

#### Organizations team invites and role administration

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] as a multi-user hosted organization feature |
| Orca ADE | [NOT EVIDENCED] in reviewed first-party sources |
| cmux | [PARTIAL] - team presence and Founder's Edition/community features; no equivalent org-admin workspace product evidenced |
| Conductor | [YES] - organizations, invitations, shared cloud workspaces, admin/team roles, billing, and enterprise controls |
| T3 Code | [PARTIAL] - account/relay-connected environments; multi-user organization admin is not a primary documented product surface |

**Comparison note:** Conductor's hosted organization model is distinct from local multi-agent coordination.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/remote-servers), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/presence-service.md), [source 4](https://www.conductor.build/docs/cloud/getting-started), [source 5](https://www.conductor.build/docs/cloud/collaboration), [source 6](https://github.com/pingdotgg/t3code/blob/main/docs/internals/t3-connect.md)

### Communication

#### Broadcast to multiple agents

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - select agents and broadcast/inject prompts through UI and CLI |
| Orca ADE | [PARTIAL] - fan-out prompt across parallel worktrees |
| cmux | [PARTIAL] - send input to selected panes/workspaces via CLI/socket |
| Conductor | [PARTIAL] - prompt together in shared workspace; broad broadcast not a core API |
| T3 Code | [PARTIAL] - multiple threads/background tasks; no dedicated broadcast command evidenced |

**Comparison note:** Wardian has a named broadcast/prompt-injection feature; Orca has fan-out.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://cmux.com/docs/cli), [source 4](https://www.conductor.build/), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Structured ask and reply handoff

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - ask/reply with durable IDs, choices, statuses, wait, and agent-facing CLI |
| Orca ADE | [PARTIAL] - orchestration messages/decision gates; exact ask/reply semantics not fully public |
| cmux | [NOT EVIDENCED] as a structured inter-agent ask/reply store |
| Conductor | [PARTIAL] - chats and collaboration; structured agent handoff not a core documented API |
| T3 Code | [PARTIAL] - messages/replies and user-input responses in orchestration runtime; public ask/reply UX not as explicit |

**Comparison note:** Wardian's ask/reply protocol is a distinctive coordination primitive.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/cli.md), [source 2](https://www.onorca.dev/docs/cli/orchestration), [source 3](https://www.conductor.build/docs/cloud/collaboration), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Manual approval request with explicit choices

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - wardian notify approval with action, risk, two-to-five choices, expiry, and durable decision |
| Orca ADE | [PARTIAL] - computer-use permission gate and provider approvals |
| cmux | [YES] - cmux Feed exposes permission, ExitPlanMode, and AskUserQuestion cards with explicit decisions bridged back to hooks |
| Conductor | [YES] - approval/permissions and workflow gates; exact Wardian notify contract not evidenced |
| T3 Code | [YES] - provider approvals and user-input responses; manual Wardian-style approval contract not evidenced |

**Comparison note:** This row distinguishes product-owned approvals from provider-native permission prompts.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/cli/computer-use), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/feed.md), [source 4](https://www.conductor.build/docs/reference/security-and-permissions), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Inbox or activity feed

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Inbox/Queue projects completions, updates, approvals, provider Action Needed, and workflow outcomes; current README uses Queue while the guide uses Inbox |
| Orca ADE | [YES] - threaded Agents feed for completions, blocking questions, unread, and worktree events |
| cmux | [YES] - notification panel/feed/sidebar attention surfaces |
| Conductor | [YES] - Home/collaboration feed, shared chats, presence, and workspace activity |
| T3 Code | [PARTIAL] - thread sidebar, messages, completion states, and usage; a Wardian-style unified Inbox projection not evidenced |

**Comparison note:** Inbox/Queue semantics are strongest in Wardian and Orca; Conductor emphasizes workspace collaboration.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/README.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 3](https://www.onorca.dev/docs/activity), [source 4](https://cmux.com/), [source 5](https://www.conductor.build/docs/cloud/collaboration), [source 6](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Unread badges and triage actions

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - persisted unread projections, badges, mark read, clear, dismiss, expand summaries |
| Orca ADE | [YES] - persistent unread bell, mark unread, notifications |
| cmux | [YES] - unread badges, attention rings, popover, and macOS notifications |
| Conductor | [PARTIAL] - presence/activity and chat; unread behavior not fully evidenced |
| T3 Code | [PARTIAL] - thread/sidebar state and settled/pinned organization; unified unread triage less evidenced |

**Comparison note:** cmux's attention ring and Wardian's durable triage are different interaction models.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/notifications), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/cloud/collaboration), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Native provider Action Needed status

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider runtime evidence becomes a distinct Action Needed item and is not generic Blocked |
| Orca ADE | [YES] - waiting/blocked status glyphs from hooks/OSC; provider-specific prompt handling less evidenced |
| cmux | [YES] - Feed hooks and Codex Teams bridge actionable provider/agent requests while preserving native-TUI fallback semantics |
| Conductor | [PARTIAL] - agent permissions and checks surface action; provider-normalized status details not evidenced |
| T3 Code | [YES] - provider approval and user-input requests are structured orchestration events |

**Comparison note:** This is a status-normalization comparison, not a claim that all products use the same status taxonomy.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/model/agents-sessions), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/feed.md), [source 4](https://www.conductor.build/docs/reference/security-and-permissions), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Agent communication graph/topology

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Graph view, manual/team-seeded edges, workspace fallback, neighbors, activity, ignore/unignore, topology.json |
| Orca ADE | [NOT EVIDENCED] as a durable agent communication graph |
| cmux | [NOT EVIDENCED] as a durable graph; workspace groups are not a relationship topology |
| Conductor | [NOT EVIDENCED] as a durable agent graph |
| T3 Code | [NOT EVIDENCED] as a product-level agent graph; orchestration relationships exist internally |

**Comparison note:** Wardian's Graph is a major unique feature.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/graph.md), [source 2](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Watchlists and live roster monitoring

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - customizable Watchlists with status lights, thought bubbles, columns, sorting, groups, teams, and lifecycle actions |
| Orca ADE | [YES] - Agent Dashboard kanban across worktrees with filters and pop-out |
| cmux | [PARTIAL] - sidebar metadata, tabs, attention, and feed; no equivalent watchlist columns evidenced |
| Conductor | [PARTIAL] - workspace Home, presence, filters, and active work; no equivalent customizable roster |
| T3 Code | [PARTIAL] - agents/thread sidebar and status; no equivalent watchlist column model evidenced |

**Comparison note:** Wardian's watchlist and thought-bubble monitoring is distinct from product/workspace feeds.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/watchlists.md), [source 2](https://www.onorca.dev/docs/model/agents-sessions), [source 3](https://www.conductor.build/docs/cloud/collaboration), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Live thought or activity telemetry beside agents

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider thought bubbles and telemetry snippets in roster/watchlists |
| Orca ADE | [PARTIAL] - status glyphs and agent feed; internal thought display not clearly documented |
| cmux | [PARTIAL] - latest notification/sidebar metadata; no agent thought bubble evidenced |
| Conductor | [NOT EVIDENCED] as internal thought display |
| T3 Code | [PARTIAL] - tool/activity messages and provider subagent model/reasoning; no thought-bubble roster evidenced |

**Comparison note:** Wardian describes this as an explicit monitoring feature; avoid conflating transcript output with private chain-of-thought.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/watchlists.md), [source 2](https://www.onorca.dev/docs/activity), [source 3](https://cmux.com/), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-codex.md)

#### Offline prompt queue and delivery receipts

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - messages can queue while an agent is offline/busy, with durable delivery receipts, replacement/withdrawal rules, and status-gated completion evidence |
| Orca ADE | [PARTIAL] - durable orchestration inbox/messages are documented; exact offline prompt receipt semantics are not fully public |
| cmux | [NOT EVIDENCED] as an offline agent-message queue; socket/terminal actions assume a live control path |
| Conductor | [PARTIAL] - shared chat/prompt workflows and workspace persistence; offline delivery receipt semantics not evidenced |
| T3 Code | [PARTIAL] - queued command metadata and durable server events exist; a user-facing offline prompt queue is not clearly documented |

**Comparison note:** Acceptance, delivery, and provider-confirmed completion are separate states in Wardian.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/native-orchestration-delivery.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/specs/2026-08-24-offline-prompt-delivery.md), [source 3](https://www.onorca.dev/docs/cli/orchestration), [source 4](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/connection-runtime.md)

#### Acceptance versus completion evidence model

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - distinguishes queued/accepted/provider-started/provider-completed evidence and reconciles uncertain submission without automatic replay |
| Orca ADE | [NOT EVIDENCED] in reviewed first-party sources |
| cmux | [NOT EVIDENCED] in reviewed first-party sources |
| Conductor | [NOT EVIDENCED] in reviewed first-party sources |
| T3 Code | [PARTIAL] - event-sourced turn state and provider receipts distinguish runtime events, but Wardian's delivery contract is not evidenced |

**Comparison note:** This is a reliability/semantics feature, not just a send button.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/native-orchestration-delivery.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/specs/2026-08-01-delivery-receipts-and-safe-mailbox-recovery.md), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Native provider delivery broker and reconciliation

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Wardian-owned delivery broker sits between adapters and mailbox/interaction stores with idempotency, generation fencing, late reconciliation, and no silent retry |
| Orca ADE | [NOT EVIDENCED] as a Wardian-style provider delivery broker |
| cmux | [NOT EVIDENCED] ; cmux is a terminal/socket primitive |
| Conductor | [NOT EVIDENCED] in reviewed first-party sources |
| T3 Code | [PARTIAL] - ProviderCommandReactor and RuntimeReceiptBus provide server-side provider dispatch and receipts, but the public semantics differ |

**Comparison note:** Adapters are transport-specific; the broker owns Wardian-level delivery semantics.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/native-orchestration-delivery.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/developer/provider-runtimes.md), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Teams and bulk selection actions

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - teams, watchlists, multi-select roster actions, broadcasts, bulk lifecycle actions, and team-seeded topology edges |
| Orca ADE | [PARTIAL] - multi-select worktrees and parallel fan-out; durable team/bulk lifecycle semantics less explicit |
| cmux | [PARTIAL] - workspace multi-selection/groups and provider teams; bulk agent lifecycle controls not central |
| Conductor | [PARTIAL] - multiple agents/workspaces and shared prompting; bulk controls not a primary documented surface |
| T3 Code | [PARTIAL] - multiple threads/workspaces and background launches; no equivalent durable team bulk-control layer evidenced |

**Comparison note:** Wardian combines selection, teams, and communication topology into one coordination model.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/specs/2026-04-19-agent-teams-and-bulk-actions.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/concepts/parallel-agents), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Feed audit log and reconnectable event stream

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - durable Inbox/interaction evidence and provider/workflow event records |
| Orca ADE | [PARTIAL] - activity feed and durable orchestration inbox |
| cmux | [YES] - Feed events and actionable/telemetry events append to workstream.jsonl; reconnectable event stream supports cursors |
| Conductor | [PARTIAL] - shared chat/transcript and workspace activity |
| T3 Code | [YES] - persisted event log and subscriptions/projectors provide replayable runtime state |

**Comparison note:** cmux's workstream audit log and T3's event log are strong matches for Wardian's evidence emphasis.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/activity), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/feed.md), [source 4](https://www.conductor.build/docs/cloud/collaboration), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Event-sourced orchestration and projections

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - durable runtime/workflow evidence and app-owned state; event-sourcing is not the primary Wardian public term |
| Orca ADE | [PARTIAL] - durable orchestration messages/tasks/inbox; implementation detail is less public |
| cmux | [PARTIAL] - workstream JSONL/events and hook state; no unified event-sourced agent runtime evidenced |
| Conductor | [PARTIAL] - cloud/workspace collaboration state; event-sourced implementation not evidenced |
| T3 Code | [YES] - commands become persisted events, projections derive read models, and subscriptions expose committed state |

**Comparison note:** T3's event-sourced model is a major architectural differentiator from cmux's socket/event primitive.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/architecture.md), [source 2](https://www.onorca.dev/docs/cli/orchestration), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/events.md), [source 4](https://www.conductor.build/docs), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Transactional command queues and drainable workers

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - deterministic workflow candidate queue and live control boundaries |
| Orca ADE | [PARTIAL] - worker/dispatch orchestration; implementation detail not fully public |
| cmux | [PARTIAL] - socket commands and event streams; queue/drain semantics not central |
| Conductor | [PARTIAL] - cloud/workspace tasks; queue implementation not evidenced |
| T3 Code | [YES] - transactional command queue, provider/runtime/checkpoint reactors, outstanding-work drain, and projection transaction boundaries |

**Comparison note:** T3 and Wardian both make execution ordering and durable work state explicit.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/workflow-engine.md), [source 2](https://www.onorca.dev/docs/cli/orchestration), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/events.md), [source 4](https://www.conductor.build/docs), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

### Coverage

#### Explicit product gaps or not-evidenced features

| Product | Assessment |
| --- | --- |
| Wardian | Unique gaps are browser depth, native diff annotation, managed cloud, public API, and third-party issue drawers relative to competitors |
| Orca ADE | Unique gaps are Wardian-style topology/Inbox/workflow builder; Orca has richer worktree IDE/review/browser depth |
| cmux | Unique gaps are agent library/orchestration/workflow/Git review; cmux is intentionally a lower-level primitive |
| Conductor | Unique gaps are local cross-platform desktop, embedded browser, provider-neutral arbitrary CLI breadth, and Wardian-style graph/library |
| T3 Code | Unique gaps are visual workflow builder, durable agent topology/watchlists, embedded browser/design mode, and rich native diff annotation |

**Comparison note:** "Not evidenced in reviewed first-party sources" means no support was found in the reviewed docs/repos; it is not a definitive negative claim.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/README.md)

### Extensibility

#### Agent-facing CLI

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - wardian CLI for agent identity, list/spawn/control, graph, workflows, library, Git, telemetry, reply, send, watch |
| Orca ADE | [YES] - Orca CLI for worktrees, terminals, browser, files/diffs, automations, artifacts, native tools, skills |
| cmux | [YES] - CLI is a primary programmable control surface |
| Conductor | [PARTIAL] - API/desktop/mobile; CLI is not the primary product surface |
| T3 Code | [YES] - t3 CLI for serve/pair/auth/connect/service and server management; app command palette handles product actions |

**Comparison note:** Wardian, Orca, cmux, and T3 all have meaningful CLI surfaces; Conductor emphasizes API.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/cli.md), [source 2](https://www.onorca.dev/docs/cli/overview), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/api), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### Machine-readable JSON or socket control

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - CLI/IPC contracts and structured evidence; app-owned state authority |
| Orca ADE | [YES] - CLI handle selectors and JSON machine interface |
| cmux | [YES] - Unix socket, JSON scripting, handles, CMUX_* identifiers |
| Conductor | [PARTIAL] - HTTP API; event/JSON details are API-owned |
| T3 Code | [YES] - authenticated Effect RPC WebSocket and JSON/event contracts |

**Comparison note:** cmux's socket/handle primitive and T3's RPC session are distinct integration approaches.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/ipc-events.md), [source 2](https://www.onorca.dev/docs/cli/reference), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/api), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Public HTTP or remote API

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - remote gateway/IPC; public automation API not a headline |
| Orca ADE | [PARTIAL] - remote server and CLI; public HTTP API not clearly documented |
| cmux | [PARTIAL] - socket/CLI; no public HTTP API evidenced |
| Conductor | [YES] - beta HTTP API for workspaces, prompts, replies, status, cancel, archive, sleep, sessions, and SQL read-only org transcripts |
| T3 Code | [PARTIAL] - server RPC is public to clients; no comparable public product HTTP API evidenced |

**Comparison note:** Conductor has the clearest documented HTTP API; T3's RPC is the internal/client protocol boundary.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/cli/reference), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/api), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### MCP integration

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - Library MCP is a stub/planned surface; provider skills can expose tools |
| Orca ADE | [YES] - skills registry/MCP and native tools |
| cmux | [PARTIAL] - cmux-artifact and related skills; MCP-style extensibility via skills |
| Conductor | [YES] - MCP stdio/Streamable HTTP/SSE at user/project/local scopes |
| T3 Code | [YES] - MCP runtime/configuration and client/server orchestration support |

**Comparison note:** MCP support ranges from first-class runtime integration to Wardian's explicitly stubbed Library section.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/library.md), [source 2](https://www.onorca.dev/docs/cli/skills), [source 3](https://github.com/manaflow-ai/cmux-skills), [source 4](https://www.conductor.build/docs/reference/mcp), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Skills registry and installation

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Garden/Library, skills deployment to agents/classes, provider-native discovery |
| Orca ADE | [YES] - installable skills and skills registry |
| cmux | [YES] - cmux-skills collection including CLI/config/browser/workspace/artifact |
| Conductor | [YES] - skills/instruction files and provider command conventions |
| T3 Code | [YES] - system/personal/project/app skills, source labels, slash menu, provider discovery |

**Comparison note:** Wardian and T3 expose skills in UI; cmux distributes a dedicated skills repository.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/library.md), [source 2](https://www.onorca.dev/docs/cli/skills), [source 3](https://github.com/manaflow-ai/cmux-skills), [source 4](https://www.conductor.build/docs/reference/agent-behavior), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Hooks and lifecycle callbacks

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider hooks/telemetry, agent notifications, workflow/listener triggers |
| Orca ADE | [YES] - .claude/.codex hooks, worktree setup hooks, managed status hooks |
| cmux | [YES] - hooks command and OSC notification protocols |
| Conductor | [PARTIAL] - setup/run/archive scripts and agent instruction files; hook API not central |
| T3 Code | [PARTIAL] - provider hooks/config and server reactors; user hook surface less explicit |

**Comparison note:** Orca and cmux make hook integration most explicit; Wardian adds workflow listeners.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/agents/hooks-memory), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/configure-your-project), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Custom commands and configurable actions

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - command panel, prompt/library actions, CLI, custom provider args |
| Orca ADE | [PARTIAL] - Quick Commands and CLI; custom action breadth not fully evidenced |
| cmux | [YES] - cmux.json custom commands, command palette, settings/config validation |
| Conductor | [PARTIAL] - slash commands, project scripts, agent instructions |
| T3 Code | [YES] - command palette, slash commands/skills, provider commands, keybindings |

**Comparison note:** cmux and T3 have explicit custom command surfaces; Wardian has a dedicated command panel.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/command-panel.md), [source 2](https://www.onorca.dev/docs/terminal), [source 3](https://cmux.com/docs/custom-commands), [source 4](https://www.conductor.build/docs/reference/slash-commands), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/keybindings.md)

#### External issue/project integrations

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - provider/GitHub integration; no documented Linear/Jira native drawer |
| Orca ADE | [YES] - GitHub, Linear, Jira, GitLab and hosted reviews |
| cmux | [NOT EVIDENCED]  |
| Conductor | [PARTIAL] - GitHub/cloud collaboration; external issue integrations not evidenced |
| T3 Code | [PARTIAL] - GitHub/GitLab/Bitbucket/Azure DevOps source-control integrations |

**Comparison note:** Orca's native issue/project integration is a notable gap for the others.
**Sources:** [source 1](https://www.onorca.dev/docs/review/github), [source 2](https://www.onorca.dev/docs/cli/orca-linear), [source 3](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Provider adapter or driver abstraction

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider-aware adapters, parsers, native delivery, session identity, and broker |
| Orca ADE | [PARTIAL] - supported agent integrations and any-CLI adapter behavior |
| cmux | [NO] - provider-neutral terminal primitive rather than provider driver layer |
| Conductor | [PARTIAL] - first-party provider support and agent modes; adapter internals not public |
| T3 Code | [YES] - built-in provider drivers, adapter registry, instance registry, command reactor, runtime ingestion |

**Comparison note:** Wardian and T3 are the most explicit provider-runtime architectures; cmux deliberately avoids this layer.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/provider-runtimes.md), [source 2](https://www.onorca.dev/docs/agents/supported), [source 3](https://www.conductor.build/docs), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Custom agent registration and fork/resume templates

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - source-backed Custom Clone flow can choose a new name, provider, class, folder, selected profile files, and skills; generic provider registration and fork/resume command templates are not evidenced |
| Orca ADE | [PARTIAL] - any-CLI support and skills; custom agent registration schema not evidenced |
| cmux | [YES] - Vault supports custom agent registration with session-ID source, resume command, fork command, session directory, and icons |
| Conductor | [NOT EVIDENCED] in reviewed first-party docs |
| T3 Code | [PARTIAL] - provider driver registry and session configuration; generic custom agent registration not evidenced |

**Comparison note:** cmux Vault is a lower-level extensibility mechanism for making arbitrary agents restorable; Wardian's related clone flow is narrower and provider-aware.
**Wardian sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/src/features/agents/CustomCloneModal.tsx), [source 2](https://github.com/wardian-app/Wardian/blob/main/src-tauri/src/commands/agent.rs)
**Sources:** [source 1](https://github.com/manaflow-ai/cmux/blob/main/docs/vault.md), [source 2](https://github.com/stablyai/orca), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Team/project skill sharing and administration

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - deploy skills globally, by class, agent, team, workspace, or workflow; no separate admin sharing service |
| Orca ADE | [YES] - skill sharing across agents/projects with admin, threat-model, and provider-path documentation |
| cmux | [PARTIAL] - cmux-skills repository and agent skills; no equivalent team admin registry evidenced |
| Conductor | [PARTIAL] - shared project instruction/context files; no skill-sharing admin system evidenced |
| T3 Code | [PARTIAL] - system/personal/project/app skill scopes; no team skill registry/admin service evidenced |

**Comparison note:** Orca is the most explicit competitor on governed skill sharing; Wardian has the richest local deployment scopes.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/library.md), [source 2](https://github.com/stablyai/orca/blob/main/docs/reference/admin-agent-skill-sharing.md), [source 3](https://github.com/stablyai/orca/blob/main/docs/reference/agent-skill-sharing-threat-model.md), [source 4](https://github.com/manaflow-ai/cmux-skills), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

### Git

#### Git status and changed-file overview

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Source Control, status, staging, diffs, change review, history |
| Orca ADE | [YES] - combined staged/unstaged/untracked diff and status |
| cmux | [PARTIAL] - sidebar branch/PR/cwd and terminal Git; no dedicated source-control panel evidenced |
| Conductor | [YES] - Diff Viewer and Checks show status, changes, PR, CI, deployments, comments, todos |
| T3 Code | [YES] - Git manager, source-control settings, branch/PR state, and changed files |

**Comparison note:** Git is universal, but cmux relies more on terminal/socket primitives.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/review/diff-viewer), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/reference/checks), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Hunk and line staging or discard

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - stage/unstage/discard changes and change-review snapshots |
| Orca ADE | [YES] - hunk/line staging, compare arbitrary refs, conflict resolution |
| cmux | [NOT EVIDENCED] as a native hunk/line staging UI |
| Conductor | [PARTIAL] - review and resolve changes; exact hunk staging not evidenced |
| T3 Code | [PARTIAL] - Git operations/source-control UI; line-level staging not evidenced |

**Comparison note:** Orca documents the richest line-level review controls.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/review/diff-viewer), [source 3](https://www.conductor.build/docs/reference/diff-viewer), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Diff viewer and arbitrary comparison refs

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - diff/change review, snapshots, history, per-agent source control |
| Orca ADE | [YES] - staged/unstaged/untracked, commit/branch/base comparisons, image diffs |
| cmux | [PARTIAL] - markdown/file previews and terminal Git; dedicated diff viewer not evidenced |
| Conductor | [YES] - unified diff viewer, changed files, unrelated edits, conflicts, review context |
| T3 Code | [PARTIAL] - source-control and review integrations; dedicated rich diff viewer not evidenced |

**Comparison note:** Orca and Conductor are strongest on first-class diff review; Wardian integrates it per agent.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/review/diff-viewer), [source 3](https://www.conductor.build/docs/reference/diff-viewer), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Inline diff comments sent back to agent

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] as a dedicated Wardian diff-comment feature |
| Orca ADE | [YES] - exact line comments, tracking, batch Send to agent, resolve/re-review |
| cmux | [NOT EVIDENCED]  |
| Conductor | [PARTIAL] - comments/review threads and agent collaboration; exact diff-line send-back not evidenced |
| T3 Code | [NOT EVIDENCED] as a dedicated diff-comment-to-agent feature |

**Comparison note:** Orca Annotate AI Diff is a distinctive review loop.
**Sources:** [source 1](https://www.onorca.dev/docs/review/annotate-ai-diff), [source 2](https://www.conductor.build/docs/reference/diff-viewer)

#### AI versus human line attribution

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - change review attributes file paths to agent turns or inferred workspace writes; individual lines are not attributed as AI or human |
| Orca ADE | [YES] - local provenance of AI/human lines and export metadata |
| cmux | [NOT EVIDENCED]  |
| Conductor | [NOT EVIDENCED]  |
| T3 Code | [NOT EVIDENCED]  |

**Comparison note:** Orca remains the only product here with explicit line-level AI/human attribution.
**Wardian sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/src-tauri/src/commands/change_review.rs), [source 2](https://github.com/wardian-app/Wardian/blob/main/src/features/changes/ChangesPanel.tsx)
**Sources:** [source 1](https://www.onorca.dev/docs/review/attribution)

#### Commit and push workflow

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - commit, pull, push, history, per-agent source control, optional worktree |
| Orca ADE | [YES] - commit/push, AI commit messages, pre-commit hooks, safe force-push lease |
| cmux | [PARTIAL] - terminal/Git CLI and remote SSH; no native commit UI evidenced |
| Conductor | [YES] - commit/merge/archive/PR workflow |
| T3 Code | [YES] - Git workflow and source-control integrations |

**Comparison note:** All can commit/push through some path; Orca and Conductor document richer integrated controls.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/review/commit-push), [source 3](https://www.conductor.build/docs/concepts/workflow), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Pull request or merge request creation

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - GitHub/review workflow exists; provider matrix not as explicit |
| Orca ADE | [YES] - GitHub/GitLab/Bitbucket/Azure DevOps/Gitea reviews and PRs/MRs |
| cmux | [PARTIAL] - branch/PR sidebar metadata and terminal workflows |
| Conductor | [YES] - PR/review/merge lifecycle and linked checks |
| T3 Code | [YES] - GitHub PRs, GitLab MRs, Bitbucket PRs, Azure DevOps PRs |

**Comparison note:** T3 and Orca explicitly document broad hosted review integrations.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/review/github), [source 3](https://www.conductor.build/docs/reference/checks), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Review checks CI deployments issues comments

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - source control, change review, telemetry, and workflow outcomes; external CI matrix not a headline |
| Orca ADE | [YES] - PR checks, hosted reviews/issues/Actions, Linear/Jira drawers |
| cmux | [PARTIAL] - PR/branch/ports/sidebar metadata and notifications |
| Conductor | [YES] - Git status, PR metadata, CI/status checks, deployments, GitHub comments/review threads, todos |
| T3 Code | [PARTIAL] - PR/review integration and linked reviews; CI/deployments breadth not fully evidenced |

**Comparison note:** Conductor explicitly combines checks, deployments, comments, and todos in one review surface.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/review/github), [source 3](https://www.conductor.build/docs/reference/checks), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Merge conflict resolution

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - change review and Git controls; explicit three-way UI not evidenced |
| Orca ADE | [YES] - 3-way merge conflict resolution in diff viewer |
| cmux | [NOT EVIDENCED] as native UI |
| Conductor | [YES] - resolve conflicts before PR/merge; exact UI detail not fully evidenced |
| T3 Code | [PARTIAL] - Git operations and review; dedicated conflict UI not evidenced |

**Comparison note:** Orca explicitly documents a three-way resolution view.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/review/diff-viewer), [source 3](https://www.conductor.build/docs/concepts/workflow), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Hosted issue and project integrations

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - GitHub/provider links; Linear/project-board integration not a core Wardian feature |
| Orca ADE | [YES] - GitHub and Linear native; Jira drawers and project boards |
| cmux | [NOT EVIDENCED]  |
| Conductor | [PARTIAL] - GitHub/cloud collaboration; Linear/Jira integration not evidenced in reviewed docs |
| T3 Code | [NOT EVIDENCED] as a native issue/project-board integration |

**Comparison note:** Orca has the strongest explicit issue/project integrations.
**Sources:** [source 1](https://www.onorca.dev/docs/review/github), [source 2](https://www.onorca.dev/docs/cli/orca-linear), [source 3](https://www.conductor.build/docs), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

### Notifications

#### Agent completion notifications

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Inbox completion cards, desktop/remote alerts, workflow outcomes |
| Orca ADE | [YES] - agent-finished pings, system/sound/chip, persistent unread |
| cmux | [YES] - OSC notifications, rings, unread, macOS notifications |
| Conductor | [PARTIAL] - workspace/chat activity and presence; completion notification details less explicit |
| T3 Code | [PARTIAL] - thread completion/status and mobile; dedicated notification matrix not evidenced |

**Comparison note:** cmux and Orca emphasize OS/attention notifications; Wardian emphasizes durable triage.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/notifications), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/cloud/collaboration), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Terminal OSC notification protocols

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - provider/runtime events and CLI notify; exact OSC matrix not a Wardian headline |
| Orca ADE | [YES] - managed status hooks/OSC support referenced |
| cmux | [YES] - OSC 9/99/777 and CLI/hooks notifications |
| Conductor | [NOT EVIDENCED]  |
| T3 Code | [PARTIAL] - provider runtime event ingestion; OSC support not evidenced |

**Comparison note:** cmux explicitly documents the OSC protocol family as a native integration point.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/agents/hooks-memory), [source 3](https://cmux.com/)

#### Sidebar status metadata branches PR cwd ports

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - roster/status/workspace/worktree/telemetry data |
| Orca ADE | [YES] - sidebar branch, PR, cwd, ports, latest notification |
| cmux | [YES] - sidebar branch, PR, cwd, ports, latest notification |
| Conductor | [PARTIAL] - workspace/project/PR/checks metadata |
| T3 Code | [PARTIAL] - thread/project/environment status; exact sidebar metadata matrix not evidenced |

**Comparison note:** Orca and cmux explicitly use a metadata-rich side rail.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/watchlists.md), [source 2](https://www.onorca.dev/docs/activity), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/reference/checks), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

### Observability

#### Agent/model/provider analytics

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Analytics matrix by agent/model/provider/time bucket and CLI summaries |
| Orca ADE | [YES] - usage/rate limit/cost tracking |
| cmux | [NOT EVIDENCED] as model/provider analytics |
| Conductor | [PARTIAL] - workspace status/checks; usage analytics not evidenced |
| T3 Code | [YES] - usage page, token/cost charts, model/provider dimensions, PostHog product analytics |

**Comparison note:** Wardian analytics is telemetry-derived; T3 additionally documents product analytics internals.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/analytics.md), [source 2](https://www.onorca.dev/docs/agents/usage-tracking), [source 3](https://www.conductor.build/docs), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/usage.md), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/product-analytics.md)

#### Resource and process diagnostics

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - telemetry, PTY/provider process observation, CPU/memory/I/O, health and lifecycle views |
| Orca ADE | [PARTIAL] - usage/resource manager setting; detailed process diagnostics not evidenced |
| cmux | [PARTIAL] - logs/sidebar state; resource diagnostics not evidenced |
| Conductor | [PARTIAL] - cloud workspace CPU/memory status |
| T3 Code | [YES] - Rust resource monitor, process tree, CPU/memory/I/O, power/thermal state, health, history, safe signaling |

**Comparison note:** T3 has unusually deep documented resource telemetry; Wardian has product telemetry and provider runtime observation.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/analytics.md), [source 2](https://www.onorca.dev/docs/settings), [source 3](https://www.conductor.build/docs/cloud/working-with-cloud-workspaces), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/resource-telemetry.md)

#### Logs and inspectable event evidence

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Markdown/filesystem state, telemetry, provider evidence, workflow timeline, raw/normalized events |
| Orca ADE | [PARTIAL] - transcript/session history and logs |
| cmux | [PARTIAL] - logs command, screen reads, screenshots, notification history |
| Conductor | [PARTIAL] - chats/transcripts/checks and cloud data |
| T3 Code | [YES] - canonical event log, orchestration projections, provider runtime ingestion, trace/diagnostic surfaces |

**Comparison note:** Wardian/T3 have the most explicit event/projection evidence models.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/ipc-events.md), [source 2](https://www.onorca.dev/docs/agents/session-history), [source 3](https://cmux.com/docs/cli), [source 4](https://www.conductor.build/docs/cloud/collaboration), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Anonymous telemetry controls and privacy

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - telemetry/analytics docs and local state; privacy control matrix not as explicit |
| Orca ADE | [YES] - anonymous product telemetry, DO_NOT_TRACK, ORCA_TELEMETRY_DISABLED |
| cmux | [NOT EVIDENCED] as product telemetry controls |
| Conductor | [YES] - security/privacy docs and cloud/local data boundaries |
| T3 Code | [YES] - product analytics boundaries, no prompts/output/paths/IP/device names in documented client metadata |

**Comparison note:** T3 and Orca explicitly document telemetry data boundaries and opt-out/config controls.
**Sources:** [source 1](https://www.onorca.dev/docs/telemetry), [source 2](https://www.conductor.build/docs/reference/security-and-permissions), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/internals/product-analytics.md)

### Projects

#### Add an existing local repository

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Explorer, project/workspace setup, and agent workspace assignment |
| Orca ADE | [YES] - create a worktree/project from a local repository |
| cmux | [YES] - open directories and create workspaces |
| Conductor | [YES] - add a local repository as a project |
| T3 Code | [YES] - Add Project from a local path/environment |

**Comparison note:** All five treat a local checkout as a first-class starting point.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/getting-started.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/first-workspace), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Clone or import hosted repositories

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - Git/source-control workflows are present; provider-specific clone matrix not a headline feature |
| Orca ADE | [YES] - GitHub and other hosted project links are integrated |
| cmux | [PARTIAL] - SSH/file open and Git terminal workflows; hosted clone matrix not evidenced |
| Conductor | [YES] - GitHub repository and repo setup; cloud GitHub integration |
| T3 Code | [YES] - GitHub, GitLab, Bitbucket, Azure DevOps, or any Git URL |

**Comparison note:** T3 has the broadest explicitly documented hosted-provider clone matrix; Conductor Cloud is GitHub-only.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/review/github), [source 3](https://www.conductor.build/docs/cloud), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Isolated Git worktree per task

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - optional per-agent worktrees with assignment, join, enable, disable, and safety checks |
| Orca ADE | [YES] - core model; every worktree has its own branch, files, terminals, and lifecycle |
| cmux | [PARTIAL] - workspace directories and SSH/tmux profiles; Git worktree orchestration not a core claim |
| Conductor | [YES] - each workspace has its own branch, working tree, terminal, diff, and review |
| T3 Code | [YES] - new worktree option per thread and server-managed project workspaces |

**Comparison note:** Worktree isolation is the clearest overlap among Wardian, Orca, Conductor, and T3; cmux is more surface-oriented.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://www.conductor.build/docs/concepts/git-worktrees), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Branch and base-ref control

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - agent worktree assignment and Git controls; provider resume is path-bound |
| Orca ADE | [YES] - choose base ref, start-from branch, commit, or remote branch |
| cmux | [PARTIAL] - branch shown in sidebar; branch management is generally terminal/Git driven |
| Conductor | [YES] - base branch, branch/workspace setup, and isolated working copy |
| T3 Code | [YES] - base branch is preserved for background threads and project workspaces |

**Comparison note:** Orca exposes the richest documented starting-ref controls.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://www.conductor.build/docs/first-workspace), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Multiple agents in one shared workspace

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - agents can share checkout/worktree; topology and watchlists expose relationships |
| Orca ADE | [PARTIAL] - parallel worktrees are the primary model; shared worktree behavior is less central |
| cmux | [YES] - multiple agents/teammates can become native panes in a workspace |
| Conductor | [YES] - multiple agents can use one shared workspace/branch |
| T3 Code | [PARTIAL] - multiple threads can target a workspace; separate worktrees are recommended for isolation |

**Comparison note:** Conductor explicitly supports both separate workspaces and shared-agent workspaces.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/concepts/parallel-agents), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Project setup scripts

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - workspace initialization and project setup exist; script contract is less central |
| Orca ADE | [PARTIAL] - worktree setup hooks and project configuration |
| cmux | [PARTIAL] - custom commands/config can automate setup; no project setup lifecycle documented |
| Conductor | [YES] - .conductor/settings.toml setup, run, archive, copies, symlinks, and generated files |
| T3 Code | [PARTIAL] - project settings and server environment setup; explicit Conductor-style setup contract not evidenced |

**Comparison note:** Conductor has the most explicit project setup contract.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/getting-started.md), [source 2](https://www.onorca.dev/docs/agents/hooks-memory), [source 3](https://github.com/manaflow-ai/cmux-skills), [source 4](https://www.conductor.build/docs/configure-your-project), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/project-settings.md)

#### Run scripts and local preview process

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - terminal/workflow shell execution and workspace launch are supported |
| Orca ADE | [PARTIAL] - terminals and port forwarding; dedicated run-script model not evidenced |
| cmux | [PARTIAL] - terminal commands and custom commands |
| Conductor | [YES] - run scripts, dev servers, watcher/test modes, and per-workspace ports |
| T3 Code | [PARTIAL] - terminal and server-side commands; project run scripts not a headline documented feature |

**Comparison note:** Conductor and T3 both expose project/runtime setup, but Conductor documents the convention more explicitly.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workflows.md), [source 2](https://www.onorca.dev/docs/ssh), [source 3](https://github.com/manaflow-ai/cmux-skills), [source 4](https://www.conductor.build/docs/concepts/testing), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Workspace environment variables

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - WARDIAN_HOME and workspace/provider settings; workflow parameters are supported |
| Orca ADE | [PARTIAL] - remote/worktree environment and provider settings |
| cmux | [PARTIAL] - cwd and shell context; CMUX_* identifiers |
| Conductor | [YES] - CONDUCTOR_WORKSPACE_* variables and setup/run environment |
| T3 Code | [YES] - server/environment variables, T3 environment IDs, and provider-scoped secrets |

**Comparison note:** All have environment context, but Conductor and T3 document product-specific variable contracts.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/cli.md), [source 2](https://www.onorca.dev/docs/ssh), [source 3](https://cmux.com/docs/cli), [source 4](https://www.conductor.build/docs/cloud/environment-variables), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### File copies symlinks and generated setup

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - workspace and worktree filesystem support; exact project-copy contract not evidenced |
| Orca ADE | [YES] - shared directories and .worktreeinclude |
| cmux | [PARTIAL] - drag/drop copy/upload and remote file handling |
| Conductor | [YES] - file copies, symlinks, generated files, and project/user settings layering |
| T3 Code | [PARTIAL] - filesystem management and uploaded attachments; project setup copy contract not evidenced |

**Comparison note:** Conductor's project configuration is unusually explicit here.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://cmux.com/docs/ssh), [source 4](https://www.conductor.build/docs/configure-your-project), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Project grouping and workspace hierarchy

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - teams, watchlists, Graph topology, and Garden districts organize agents |
| Orca ADE | [YES] - projects, parent/child worktrees, grouping, pinning, archiving, and filters |
| cmux | [PARTIAL] - workspace groups and tabs; no worktree hierarchy equivalent evidenced |
| Conductor | [PARTIAL] - projects and workspace lists; hierarchy beyond project/workspace not evidenced |
| T3 Code | [PARTIAL] - projects, environments, thread sidebar, pinned/settled threads; no worktree hierarchy evidenced |

**Comparison note:** Orca has the richest documented worktree hierarchy; Wardian has richer ecological/topological grouping.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/garden.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/guide/watchlists.md), [source 3](https://www.onorca.dev/docs/model/worktrees), [source 4](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 5](https://www.conductor.build/docs/concepts/workspaces-and-branches), [source 6](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Archive sleep delete and lifecycle cleanup

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - agent/worktree delete and lifecycle controls with safety boundaries |
| Orca ADE | [YES] - archive/delete worktrees; hibernate idle agents |
| cmux | [PARTIAL] - close/restore panes and session state; Git workspace cleanup not a core feature |
| Conductor | [YES] - archive, merge, sleep/wake cloud workspaces, and review lifecycle |
| T3 Code | [YES] - archive/sleep/unarchive environments and settle threads |

**Comparison note:** All expose cleanup, but the unit differs: agent, worktree, pane, workspace, or environment.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/source-control.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://www.onorca.dev/docs/agents/hibernation), [source 4](https://cmux.com/), [source 5](https://www.conductor.build/docs/api), [source 6](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Workspace status comments and checkpoints

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - agent/workflow state and source-control status; checkpoint semantics are workflow/runtime focused |
| Orca ADE | [YES] - free-text worktree checkpoint comment plus todo/in-progress/in-review/completed status |
| cmux | [PARTIAL] - sidebar status, progress, logs, and notifications; no worktree checkpoint model evidenced |
| Conductor | [PARTIAL] - workspace status, checks, PR state, and review lifecycle; free-text checkpoint field not evidenced |
| T3 Code | [YES] - persistent checkpoints and checkpoint revert in orchestration runtime |

**Comparison note:** Checkpoint is a clear T3 feature; Orca has a similar worktree checkpoint UX.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/cli/worktree-checkpoints), [source 3](https://www.conductor.build/docs/reference/checks), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Search filter pin and multi-select organization

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - agent filters, Watchlists, Graph scopes, Library search, Workbench tabs, and roster selection |
| Orca ADE | [YES] - search/filter by project/status/PR, pin/archive, multi-select |
| cmux | [PARTIAL] - quick open, jump palette, workspace groups, and sidebar filters |
| Conductor | [YES] - Home filters, pinned/My work, teammate/project/archived views |
| T3 Code | [YES] - thread sidebar, pinned/settled organization, environment selection, and filters |

**Comparison note:** All have navigation/filtering; Wardian and Orca emphasize agent/worktree organization most strongly.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/watchlists.md), [source 2](https://www.onorca.dev/docs/model/agents-sessions), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/cloud/collaboration), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Workspace groups and collapsible organization

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - watchlists, teams, Garden districts, and Workbench panes organize agent work |
| Orca ADE | [YES] - project grouping, parent/child worktrees, pin/archive/filter |
| cmux | [YES] - collapsible workspace groups with anchor workspaces, pinning, reorder, and delete/ungroup |
| Conductor | [PARTIAL] - projects and workspace lists; no equivalent group-anchor model evidenced |
| T3 Code | [PARTIAL] - project groups/environments and thread sidebar; no equivalent workspace-group model evidenced |

**Comparison note:** cmux's workspace group is a unique sidebar hierarchy; Wardian's grouping is agent/workflow/ecological rather than workspace-anchor based.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/garden.md), [source 2](https://www.onorca.dev/docs/model/worktrees), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/workspace-groups.md), [source 4](https://www.conductor.build/docs/concepts/workspaces-and-branches), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Workspace auto-naming from agent conversation

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - agent descriptions and contextual titles; automatic topic summarization not a headline feature |
| Orca ADE | [PARTIAL] - session/worktree naming and repo context; automatic conversation naming not clearly documented |
| cmux | [YES] - opt-in AI workspace/tab naming from conversation content with user-name precedence |
| Conductor | [PARTIAL] - workspace/thread titles and regenerated titles; auto naming details not central |
| T3 Code | [YES] - thread title generation/regeneration and environment artwork/title surfaces |

**Comparison note:** cmux and T3 explicitly document automatic conversation-derived naming; Wardian relies more on durable descriptions.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/agents-overview.md), [source 2](https://github.com/manaflow-ai/cmux/blob/main/docs/workspace-auto-naming.md), [source 3](https://www.conductor.build/docs), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Workspace todos checklist and status lanes

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - workflow nodes, Inbox outcomes, watchlists, and agent/workflow statuses |
| Orca ADE | [PARTIAL] - worktree checkpoint status and agent dashboard lanes |
| cmux | [YES] - workspace todo pane/checklists and todo/working/needs-attention/review/done status lanes with notifications |
| Conductor | [PARTIAL] - checks/todos/review state; no equivalent sidebar checklist contract evidenced |
| T3 Code | [PARTIAL] - thread states, checkpoints, and review status; no equivalent workspace checklist surface evidenced |

**Comparison note:** cmux's status lane is workspace-level; Orca's similar checkpoint is worktree-level.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/watchlists.md), [source 2](https://github.com/stablyai/orca/blob/main/docs/reference/orcad-operations.md), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/configuration.md), [source 4](https://www.conductor.build/docs/reference/checks), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Project icon detection and customization

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] as a Wardian product feature |
| Orca ADE | [PARTIAL] - repo/worktree previews and project grouping; icon picker not evidenced |
| cmux | [NOT EVIDENCED] in reviewed first-party sources |
| Conductor | [PARTIAL] - project/workspace identity and artwork; file-based icon picker not evidenced |
| T3 Code | [YES] - auto-detects t3.json/favicon/app-icon paths and lets users choose SVG/PNG/ICO/JPEG/GIF/AVIF/WebP project files across checkouts/clients |

**Comparison note:** T3's project icon system is a small but explicit project-management feature.
**Sources:** [source 1](https://github.com/pingdotgg/t3code/blob/main/docs/user/project-settings.md), [source 2](https://www.conductor.build/docs/concepts/workspaces-and-branches)

### Remote

#### SSH remote workspaces

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - remote control and remote agent/workspace handoff; exact remote file/PTY breadth is narrower |
| Orca ADE | [YES] - SSH worktrees with remote file/git/editor/diff/browser, reconnect, downloads, port forwarding |
| cmux | [YES] - SSH workspaces, attach remote tmux, remote browser, scp uploads, reconnect |
| Conductor | [PARTIAL] - local/cloud split and SSH/file sync; remote SSH workspaces are less central |
| T3 Code | [YES] - desktop-managed SSH launch, remote server owns projects/files/Git/terminals/providers |

**Comparison note:** All five address remote execution; Orca/cmux/T3 document the richest SSH-specific details.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/ssh), [source 3](https://cmux.com/docs/ssh), [source 4](https://www.conductor.build/docs/cloud), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### Headless server or daemon

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - app-owned backend/CLI and remote gateway; no Wardian headless server product claim |
| Orca ADE | [YES] - orca serve and Remote Orca Server |
| cmux | [PARTIAL] - remote SSH/tmux; no separate cmux headless server claim |
| Conductor | [YES] - Cloud workspaces continue after laptop closes; local app is controller |
| T3 Code | [YES] - t3 serve headless server and background service |

**Comparison note:** T3 and Orca explicitly expose headless/daemon paths.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/remote-servers), [source 3](https://www.conductor.build/docs/cloud), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### Managed remote cloud workspace or microVM

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED]  |
| Orca ADE | [PARTIAL] - disposable cloud VM/per-workspace environment |
| cmux | [PARTIAL] - Cloud VMs are listed as early access in the Founder's Edition; general availability is not documented |
| Conductor | [YES] - isolated Conductor Cloud microVM/sandbox with repos/deps/secrets/software |
| T3 Code | [NOT EVIDENCED] as managed execution; T3 Connect exposes a server rather than a coding sandbox |

**Comparison note:** Conductor is the only generally documented managed isolated workspace platform; cmux's cloud VM capability is early access.
**Sources:** [source 1](https://www.onorca.dev/docs/ways-to-run), [source 2](https://github.com/manaflow-ai/cmux), [source 3](https://www.conductor.build/docs/cloud), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/t3-connect.md)

#### Remote collaboration and presence

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - topology/watchlists/remote roster; shared multiplayer editing not a core claim |
| Orca ADE | [PARTIAL] - mobile/remote agents and shared server; multiplayer presence not strongly documented |
| cmux | [YES] - device presence service lets team members see who is live/offline; remote SSH notification routing is also supported |
| Conductor | [YES] - share workspace link, presence, follow, prompt together, shared chats/transcripts |
| T3 Code | [PARTIAL] - shared environments and account-connected clients; collaboration features less central |

**Comparison note:** Conductor's multiplayer positioning is strongest; cmux presence is a separate team service.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/activity), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/presence-service.md), [source 4](https://cmux.com/), [source 5](https://www.conductor.build/), [source 6](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Port forwarding and live preview routing

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - remote control and browser surfaces; port-forwarding contract not a headline |
| Orca ADE | [YES] - SSH port forwarding and remote browser/preview |
| cmux | [YES] - SSH reverse TCP relay and browser routing |
| Conductor | [YES] - auto-port-forwarding and workspace port workflows |
| T3 Code | [YES] - Tailscale endpoints, SSH local port forward, server/environment endpoints |

**Comparison note:** Conductor and Orca explicitly emphasize ports; T3 documents endpoints/forwarding primarily for remote access.
**Sources:** [source 1](https://www.onorca.dev/docs/ssh), [source 2](https://cmux.com/docs/ssh), [source 3](https://www.conductor.build/), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### Remote file transfer and upload/download

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - remote filesystem and agent/file surfaces |
| Orca ADE | [YES] - remote file/folder download and drag files to agents |
| cmux | [YES] - scp drag image upload and remote file workflows |
| Conductor | [PARTIAL] - sync local directory and cloud workspace files |
| T3 Code | [PARTIAL] - attachments upload to environment; SSH/file operations are server-side |

**Comparison note:** cmux and Orca explicitly document remote transfer gestures; T3 documents server-side attachment upload.
**Sources:** [source 1](https://www.onorca.dev/docs/ssh), [source 2](https://cmux.com/docs/ssh), [source 3](https://www.conductor.build/docs/cloud/working-with-cloud-workspaces), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Phone or browser pairing and remote gateway

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Tailscale Serve remote gateway, paired PWA, revocation, CSRF/origin checks |
| Orca ADE | [YES] - mobile companion and remote servers; pairing details are product-specific |
| cmux | [YES] - iOS pairing beta |
| Conductor | [YES] - share workspace link and desktop/mobile/API clients |
| T3 Code | [YES] - one-time pairing tokens/QR, LAN/Tailscale/HTTPS endpoints, hosted web pairing |

**Comparison note:** T3 and Wardian both document explicit device pairing/security flows.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/mobile), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### Tailscale or tunnel transport

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Tailscale Serve is the v1 remote-control path |
| Orca ADE | [PARTIAL] - Tailscale/remote server support is possible; not the core docs path |
| cmux | [PARTIAL] - SSH relay and remote browser; Tailscale is not core |
| Conductor | [PARTIAL] - cloud service; self-hosted/tunnel details not central |
| T3 Code | [YES] - Tailscale IP/Serve HTTPS, relay/T3 Connect, and direct LAN/custom HTTPS |

**Comparison note:** Tailscale is explicit in Wardian and T3; cmux relies more on SSH relay.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/remote-servers), [source 3](https://cmux.com/docs/ssh), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### Background service and unattended host

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] as a separate installed service |
| Orca ADE | [PARTIAL] - daemon/Remote Orca Server; service lifecycle not fully documented |
| cmux | [NOT EVIDENCED]  |
| Conductor | [YES] - cloud workspaces persist independently of laptop; local service not central |
| T3 Code | [YES] - Linux systemd/macOS launch agent, install/update/uninstall; Windows unsupported |

**Comparison note:** T3 explicitly documents background service support and its platform limitation.
**Sources:** [source 1](https://www.onorca.dev/docs/remote-servers), [source 2](https://www.conductor.build/docs/cloud), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/user/background-service.md)

#### Cloud environment resources secrets and software

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED]  |
| Orca ADE | [PARTIAL] - disposable VM/per-workspace environment |
| cmux | [NOT EVIDENCED]  |
| Conductor | [YES] - Cloud Computer with shared repos/env/secrets/software/builds and specified resources |
| T3 Code | [NOT EVIDENCED] as a managed shared cloud-computer feature |

**Comparison note:** Conductor Cloud Computer is a distinctive product capability.
**Sources:** [source 1](https://www.onorca.dev/docs/ways-to-run), [source 2](https://www.conductor.build/docs/cloud/cloud-computer), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/internals/t3-connect.md)

#### Client/server version coordination

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - app/CLI/provider compatibility and persisted state; explicit client/server skew flow not a headline |
| Orca ADE | [PARTIAL] - remote server/client and app updates; details not as explicit |
| cmux | [PARTIAL] - session restore/reconnect; version coordination not evidenced |
| Conductor | [PARTIAL] - desktop/cloud product updates; explicit skew model not evidenced |
| T3 Code | [YES] - connection warnings, server update/reconnect flow, stable environment ID, version coordination |

**Comparison note:** T3 has the most explicit client/server synchronization docs.
**Sources:** [source 1](https://www.onorca.dev/docs/remote-servers), [source 2](https://www.conductor.build/docs), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/user/updating.md), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Remote tmux attachment

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - remote terminal handoff and paired PWA; native tmux attach not a headline feature |
| Orca ADE | [PARTIAL] - SSH worktrees and remote PTYs; tmux attach not a headline feature |
| cmux | [YES] - SSH workspaces can attach remote tmux sessions and preserve/reconnect them |
| Conductor | [PARTIAL] - SSH/file sync and cloud workspaces; tmux attach not central |
| T3 Code | [PARTIAL] - desktop-managed SSH launch starts/reuses T3 server; remote tmux attach is not the primary model |

**Comparison note:** cmux explicitly supports tmux as a remote attachment target; T3 runs its own server runtime.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/ssh), [source 3](https://cmux.com/docs/remote-tmux), [source 4](https://www.conductor.build/docs/cloud), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### Mobile terminal synchronization

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - remote PWA controls paired terminal/chat sessions |
| Orca ADE | [YES] - mobile companion monitors and steers desktop agents |
| cmux | [EXPERIMENTAL] - iOS beta and Founder's Edition describe terminals synced between desktop and phone |
| Conductor | [PARTIAL] - mobile app and cloud workspace access; exact terminal-sync scope is evolving |
| T3 Code | [PARTIAL] - mobile client and remote environments are implemented, but distribution is still in development |

**Comparison note:** Availability is intentionally recorded separately from code presence.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/mobile), [source 3](https://github.com/manaflow-ai/cmux#founders-edition), [source 4](https://www.conductor.build/), [source 5](https://github.com/pingdotgg/t3code/blob/main/apps/mobile/README.md)

#### Headless Linux/server mode

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - app-owned backend and CLI can support remote control; no separate Wardian headless server product is documented |
| Orca ADE | [YES] - headless Linux server via orca serve and remote Orca Server |
| cmux | [PARTIAL] - remote daemon/cloud VM work is documented, but cmux desktop remains the primary product |
| Conductor | [PARTIAL] - cloud workspaces run independently of the laptop |
| T3 Code | [YES] - t3 serve headless server and Linux background service |

**Comparison note:** Headless/server mode is explicit for Orca and T3; Conductor's cloud is managed workspace execution rather than a BYO headless server.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://github.com/stablyai/orca/blob/main/docs/reference/headless-linux-server.md), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cloud-cmux-tui-daemon.md), [source 4](https://www.conductor.build/docs/cloud), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### WSL and cross-host shell execution

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - cross-platform PTY/process model; WSL is not a headline Wardian product mode |
| Orca ADE | [YES] - Windows setup, WSL command execution, and remote execution boundary docs |
| cmux | [NO] - native macOS desktop; remote SSH is the cross-host path |
| Conductor | [NO] - local desktop is macOS; cloud execution is remote Linux |
| T3 Code | [YES] - Windows desktop can use a WSL backend; SSH and headless Linux are also supported |

**Comparison note:** This row captures where a product can run provider commands, not where its UI is installed.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/pty-lifecycle.md), [source 2](https://github.com/stablyai/orca/blob/main/docs/reference/wsl-command-execution.md), [source 3](https://cmux.com/docs/ssh), [source 4](https://www.conductor.build/docs/installation), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md)

#### Connection runtime offline cache and reconnect

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - paired remote PWA, cached roster/startup shell, and explicit offline action limits |
| Orca ADE | [PARTIAL] - remote daemon/reconnect and session restoration |
| cmux | [PARTIAL] - SSH reconnect and mobile/network reliability; state model is app-specific |
| Conductor | [PARTIAL] - cloud workspace wake/reconnect and persistent workspaces |
| T3 Code | [YES] - shared web/mobile connection runtime with environment registry, direct/bearer/relay/Tailscale/SSH targets, cached shell/thread snapshots, retry, sync states, and explicit removal |

**Comparison note:** T3 documents the most systematic client connection state machine; Wardian documents a narrower remote-control surface.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/ssh), [source 3](https://cmux.com/docs/ssh), [source 4](https://www.conductor.build/docs/cloud/working-with-cloud-workspaces), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/connection-runtime.md)

#### T3 Connect relay and account-linked environment management

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - remote pairing/gateway but no Wardian account relay product |
| Orca ADE | [PARTIAL] - Remote Orca Server and relay/remote hosting |
| cmux | [PARTIAL] - relay/SSH/cloud work; account-linked environment catalog not central |
| Conductor | [PARTIAL] - cloud account/org environment management |
| T3 Code | [YES] - Clerk-authenticated T3 Connect login/link/unlink/logout, managed cloudflared relay, deregistration, environment catalog, and mobile/web/desktop linking |

**Comparison note:** T3 Connect is a managed connectivity layer, not managed code execution.
**Sources:** [source 1](https://github.com/pingdotgg/t3code/blob/main/docs/internals/t3-connect.md), [source 2](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md), [source 3](https://www.conductor.build/docs/api)

### Scope

#### Comparison identity

| Product | Assessment |
| --- | --- |
| Wardian | Wardian desktop and CLI habitat for multiple agent sessions and workflows |
| Orca ADE | Orca ADE by Stably AI; worktree-native agent IDE |
| cmux | cmux by Manaflow AI; native terminal and browser workspace primitive |
| Conductor | Conductor by Melty Labs; parallel-agent workspace product |
| T3 Code | T3 Code by pingdotgg; server-backed GUI for coding agents |

**Comparison note:** Conductor is scoped to conductor.build rather than Microsoft Conductor. Statuses describe first-party documentation reviewed on 2026-08-29.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://github.com/stablyai/orca), [source 3](https://github.com/manaflow-ai/cmux), [source 4](https://www.conductor.build/), [source 5](https://github.com/pingdotgg/t3code)

#### What the product is

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - agent habitat with inspectable state, desktop UI, CLI, Library, Graph, Inbox, Workflows, Explorer, and Workbench |
| Orca ADE | [YES] - worktree IDE for running and reviewing coding agents |
| cmux | [YES] - terminal, browser, workspaces, splits, tabs, notifications, and programmable control; intentionally a primitive |
| Conductor | [YES] - desktop and cloud workspace manager for parallel coding agents |
| T3 Code | [YES] - web and desktop GUI with a server runtime, projects, threads, terminals, files, and Git |

**Comparison note:** This row is a product-level comparison; later rows decompose the surfaces.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Primary execution model

| Product | Assessment |
| --- | --- |
| Wardian | Local-first Tauri/Rust backend with provider-aware PTY and native transports; remote control is an added surface |
| Orca ADE | Local desktop, remote SSH worktrees, remote Orca servers, and optional cloud VMs |
| cmux | Native macOS app with local and SSH/tmux-backed workspaces |
| Conductor | Local workspaces plus isolated Conductor Cloud sandboxes |
| T3 Code | Node server runtime locally, headlessly, remotely, or behind T3 Connect |

**Comparison note:** Local versus hosted and managed-cloud boundaries are important differences.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/ways-to-run), [source 3](https://cmux.com/docs/ssh), [source 4](https://www.conductor.build/docs/cloud), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Supported operating systems

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - desktop/runtime targets are cross-platform; remote PWA covers phones |
| Orca ADE | [YES] - macOS, Windows, Linux desktop; iOS and Android companion |
| cmux | [NO] - for the desktop app - macOS native; iOS app is beta |
| Conductor | [NO] - for the local desktop app - macOS only; Cloud Computer is remote Linux |
| T3 Code | [YES] - Windows, macOS, and Arch Linux desktop; Linux server; WSL backend |

**Comparison note:** Conductor local macOS-only support is an explicit limitation; cmux desktop macOS-only is also explicit.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://github.com/stablyai/orca), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/installation), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md)

#### License and openness

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - public repository under the MIT license |
| Orca ADE | [YES] - MIT |
| cmux | [YES] - GPL |
| Conductor | [NOT EVIDENCED] in reviewed first-party product docs |
| T3 Code | [YES] - MIT in the repository metadata |

**Comparison note:** License is included where the first-party repository or site states it; Wardian's repository metadata reports a public MIT-licensed repository.
**Wardian sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/LICENSE), [source 2](https://github.com/wardian-app/Wardian/blob/main/package.json), [source 3](https://github.com/wardian-app/Wardian/blob/main/src-tauri/Cargo.toml)
**Sources:** [source 1](https://github.com/stablyai/orca), [source 2](https://github.com/manaflow-ai/cmux), [source 3](https://github.com/pingdotgg/t3code)

#### Core architecture boundary

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Rust/Tauri owns lifecycle, PTY, filesystem, telemetry, workflow engine, and provider runtime authority |
| Orca ADE | [NOT EVIDENCED] as a formal architecture boundary; docs expose desktop daemon and worktree model |
| cmux | [YES] - Swift/AppKit plus libghostty, with a socket/CLI control plane |
| Conductor | [YES] - app manages workspaces and agents; Cloud adds isolated sandbox infrastructure |
| T3 Code | [YES] - Node WebSocket server, React clients, provider drivers, orchestration event loop, and Rust resource monitor |

**Comparison note:** Architecture detail is stronger for Wardian and T3 than for Orca and Conductor public product docs.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/architecture.md), [source 2](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/resource-telemetry.md)

#### Local-first Markdown or file truth

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - prompts, classes, skills, workflows, results, topology, and state are inspectable on disk |
| Orca ADE | [PARTIAL] - reads agent transcripts and project files; no equivalent Wardian library truth model evidenced |
| cmux | [PARTIAL] - config, hooks, custom sidebars, and session state are file-backed; no equivalent Library model evidenced |
| Conductor | [PARTIAL] - project configuration and context files are file-backed; workspace state is app-managed |
| T3 Code | [PARTIAL] - server/database/event projections own runtime state; project files remain local to the environment |

**Comparison note:** This distinguishes inspectable project artifacts from canonical runtime state.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/agents/hooks-memory), [source 3](https://github.com/manaflow-ai/cmux-skills), [source 4](https://www.conductor.build/docs/configure-your-project), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Managed cloud execution

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] as a Wardian-managed cloud product; BYO remote control is documented |
| Orca ADE | [PARTIAL] - disposable cloud VM/per-workspace environment is documented; no managed VPS |
| cmux | [PARTIAL] - Cloud VMs are listed as early access in the cmux Founder's Edition; general availability is not documented |
| Conductor | [YES] - Conductor Cloud isolated microVMs/sandboxes with repos, dependencies, secrets, and persistence |
| T3 Code | [PARTIAL] - T3 Connect relay/cloud service connects environments; execution remains on the linked server |

**Comparison note:** Cloud execution versus cloud connectivity is a major axis; cmux cloud is availability-gated rather than a confirmed general release.
**Sources:** [source 1](https://www.onorca.dev/docs/ways-to-run), [source 2](https://github.com/manaflow-ai/cmux), [source 3](https://www.conductor.build/docs/cloud), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/t3-connect.md)

#### BYO provider CLI model

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - supports provider CLIs plus Wardian-native provider transports |
| Orca ADE | [YES] - any CLI can run; first-class support is layered over agent CLIs |
| cmux | [YES] - any CLI in terminal panes; native pane behavior is CLI/provider agnostic |
| Conductor | [PARTIAL] - first-party agent integrations plus local agent commands |
| T3 Code | [YES] - drives provider CLIs installed and authenticated on the server |

**Comparison note:** Orca and T3 explicitly document any CLI/provider-CLI use; cmux is the most provider-agnostic.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/agents/supported), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/agent-behavior), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md)

#### Web desktop and mobile client surfaces

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Tauri desktop plus paired remote PWA/mobile browser surface |
| Orca ADE | [YES] - desktop, mobile companion, and remote/headless server |
| cmux | [YES] - native macOS desktop and beta iOS app |
| Conductor | [YES] - macOS desktop, mobile, and API/cloud access |
| T3 Code | [YES] - web app, Electron desktop, mobile app, and headless/server clients; mobile distribution is still in development |

**Comparison note:** Client count is not the same as execution location; T3 and Wardian separate client surfaces from server/runtime authority.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/README.md), [source 2](https://github.com/stablyai/orca), [source 3](https://github.com/manaflow-ai/cmux), [source 4](https://www.conductor.build/), [source 5](https://github.com/pingdotgg/t3code/blob/main/README.md)

### Security

#### Local permissions and cloud sandbox boundary

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - local app authority, provider permissions, remote gateway security, app-owned workflow state |
| Orca ADE | [PARTIAL] - computer-use permission gate and local/remote modes |
| cmux | [PARTIAL] - local native app, SSH relay, browser automation state |
| Conductor | [YES] - local user permissions versus isolated cloud sandbox; enterprise/security docs |
| T3 Code | [YES] - provider permission modes, server-side auth, pairing/session security, local versus remote environment |

**Comparison note:** Conductor and T3 make execution boundaries explicit; Wardian separates provider approvals from workflow/manual decisions.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/cli/computer-use), [source 3](https://www.conductor.build/docs/reference/security-and-permissions), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/permission-modes.md)

#### Pairing tokens sessions and revocation

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - paired remote sessions, revocation/reset, origin/CSRF controls |
| Orca ADE | [PARTIAL] - mobile/remote server access; exact token lifecycle not evidenced |
| cmux | [PARTIAL] - SSH relay HMAC/reconnect; iOS pairing details not fully public |
| Conductor | [PARTIAL] - shared workspace links and org access; token lifecycle not detailed |
| T3 Code | [YES] - one-time owner pairing token, authenticated sessions, auth command, revoke/deregister, hosted hash token |

**Comparison note:** T3 documents the most complete explicit pairing and revocation lifecycle; Wardian documents a secure paired PWA path.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/remote-servers), [source 3](https://cmux.com/docs/ssh), [source 4](https://www.conductor.build/docs/reference/security-and-permissions), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### Provider credential isolation

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider settings/instances and runtime-owned credentials |
| Orca ADE | [PARTIAL] - account switcher and provider homes |
| cmux | [NOT EVIDENCED]  |
| Conductor | [YES] - own subscriptions/API keys, cloud secrets, org/admin boundaries |
| T3 Code | [YES] - provider homes/config dirs, shadow homes, sensitive env vars, server-side authentication |

**Comparison note:** T3's provider-home/shadow-home model is a distinctive account isolation feature.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/provider-readiness.md), [source 2](https://www.onorca.dev/docs/agents/usage-tracking), [source 3](https://www.conductor.build/docs/cloud/cloud-computer), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-codex.md)

#### SSH host-key verification and remote execution boundary

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - remote gateway and paired-origin controls; SSH host-key semantics are not central |
| Orca ADE | [YES] - known-host/FIDO/GSSAPI and host-key verification/execution-boundary documentation |
| cmux | [YES] - SSH relay, remote network browser, reconnect, and authenticated remote transport |
| Conductor | [PARTIAL] - cloud sandbox and SSH/file sync security; detailed host-key path not central |
| T3 Code | [YES] - SSH launch, remote server ownership, auth/readiness checks, and explicit security/session boundaries |

**Comparison note:** Orca and T3 document SSH execution boundaries most explicitly; cmux documents relay/reconnect behavior.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://github.com/stablyai/orca/blob/main/docs/reference/ssh-host-key-verification.md), [source 3](https://github.com/stablyai/orca/blob/main/docs/reference/ssh-execution-boundary.md), [source 4](https://cmux.com/docs/ssh), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md)

#### Enterprise provisioning privacy and SLA controls

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] as a Wardian enterprise control plane |
| Orca ADE | [NOT EVIDENCED] in reviewed first-party sources |
| cmux | [NOT EVIDENCED] in reviewed first-party sources |
| Conductor | [YES] - enterprise provisioning, fine-grained security, privacy/data boundary documentation, and SLA offerings |
| T3 Code | [NOT EVIDENCED] as an enterprise plan/control plane in reviewed repository docs |

**Comparison note:** Conductor is the only product here with a clearly documented enterprise control layer.
**Sources:** [source 1](https://www.conductor.build/docs/cloud), [source 2](https://www.conductor.build/docs/reference/security-and-permissions), [source 3](https://www.conductor.build/docs/reference/privacy)

#### Short-lived WebSocket authentication tickets

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - remote gateway session/auth controls; exact WebSocket-ticket model not evidenced |
| Orca ADE | [PARTIAL] - remote-server auth; exact ticket semantics not evidenced |
| cmux | [PARTIAL] - SSH/relay/HMAC transport auth; no equivalent WebSocket ticket docs |
| Conductor | [PARTIAL] - cloud account/sandbox auth; exact ticket semantics not evidenced |
| T3 Code | [YES] - dedicated short-lived WebSocket tickets after authenticated pairing, method scopes, bearer/relay/SSH target authorization |

**Comparison note:** This is a T3-specific server security boundary.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://cmux.com/docs/ssh), [source 3](https://www.conductor.build/docs/reference/security-and-permissions), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

### Settings

#### Themes fonts density and keyboard customization

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - theme sync, semantic tokens, shell/theme settings, command shortcuts |
| Orca ADE | [YES] - themes/customization, Ghostty/Warp theme import, UI zoom/density/fonts/minimap |
| cmux | [YES] - Ghostty config, themes/fonts/cursor, settings/keybindings |
| Conductor | [PARTIAL] - settings and appearance; full theme/keybinding matrix not evidenced |
| T3 Code | [YES] - settings, keybindings JSON, themes/appearance, environment artwork/version pill |

**Comparison note:** cmux and Orca expose terminal/theme customization; T3 exposes configurable keybindings; Wardian uses semantic theming.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/settings.md), [source 2](https://www.onorca.dev/docs/settings), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/keybindings.md)

#### Shell selection and terminal presets

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - shell selection and runtime/session policy controls |
| Orca ADE | [YES] - PowerShell/CMD/WSL, terminal presets, Quick Commands |
| cmux | [YES] - Ghostty configuration and shell terminals |
| Conductor | [YES] - terminal presets and Big Terminal Mode |
| T3 Code | [YES] - server-side shell/provider environment and terminal sessions |

**Comparison note:** Windows shell coverage is explicit for Orca and T3; Wardian also models shell selection.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/features.md), [source 2](https://www.onorca.dev/docs/terminal), [source 3](https://www.conductor.build/docs/reference/big-terminal-mode), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md)

#### Provider environment variables and secret handling

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider/runtime settings with safe configuration boundaries |
| Orca ADE | [PARTIAL] - provider settings/accounts; secret storage details not fully evidenced |
| cmux | [PARTIAL] - config files and environment; secret management not central |
| Conductor | [YES] - cloud/local env/secrets and provider credentials |
| T3 Code | [YES] - provider-scoped env vars, sensitive secret storage, account endpoints, auth settings |

**Comparison note:** T3 and Conductor explicitly document secret handling; Wardian's provider readiness docs cover configuration boundaries.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/provider-readiness.md), [source 2](https://www.onorca.dev/docs/settings), [source 3](https://www.conductor.build/docs/cloud/cloud-computer), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/providers-claude.md)

#### App updates and release channels

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - app/runtime updates and persisted state; channel controls not a key Wardian feature |
| Orca ADE | [PARTIAL] - updates/RC settings and rollback behavior |
| cmux | [PARTIAL] - native app updates; details not a core feature |
| Conductor | [PARTIAL] - cloud/desktop release updates |
| T3 Code | [YES] - updating docs, client/server version warnings, stable/remote candidate handling |

**Comparison note:** T3 and Orca document update workflows more explicitly than cmux/Conductor public docs.
**Sources:** [source 1](https://www.onorca.dev/docs/settings), [source 2](https://www.conductor.build/docs), [source 3](https://github.com/pingdotgg/t3code/blob/main/docs/user/updating.md), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/background-service.md)

#### Guided tour and contextual onboarding guidance

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - first-run guided tour, dismissible contextual tips, reset guidance, provider readiness setup, and troubleshooting |
| Orca ADE | [NOT EVIDENCED] in reviewed first-party sources |
| cmux | [NOT EVIDENCED] in reviewed first-party sources |
| Conductor | [PARTIAL] - first workspace onboarding and cloud setup |
| T3 Code | [PARTIAL] - install/first-run/provider setup docs; in-app guided-tour feature not evidenced |

**Comparison note:** Wardian's onboarding is an in-app habitat setup feature, not just installation documentation.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/getting-started.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/guide/settings.md), [source 3](https://www.conductor.build/docs/first-workspace), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/install.md)

#### External editor and native file-manager integration

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - per-file-family opening preferences, VS Code/custom executable, system default app, reveal in Explorer/Finder, and copy path |
| Orca ADE | [PARTIAL] - editor/file explorer and external app workflows; exact preference matrix not evidenced |
| cmux | [PARTIAL] - open files/dirs and drag/drop; external editor matrix not evidenced |
| Conductor | [NOT EVIDENCED] in reviewed first-party sources |
| T3 Code | [PARTIAL] - desktop/server file operations; external editor preference not evidenced |

**Comparison note:** This is a local filesystem integration gap in most competitors.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/explorer.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/guide/settings.md), [source 3](https://www.onorca.dev/docs/editing/file-explorer), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

### Surfaces

#### Workbench or multi-surface shell

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Workbench composes agents, terminals, files, Inbox, workflows, Graph, and Source Control |
| Orca ADE | [YES] - worktree IDE with terminal/editor/browser/diff/PR tabs |
| cmux | [YES] - native workspaces with terminal/browser/markdown/file-preview surfaces |
| Conductor | [YES] - workspace-centric UI with chat, terminal, diff, checks, and PR |
| T3 Code | [YES] - web/desktop/mobile clients around one server runtime with thread/sidebar surfaces |

**Comparison note:** Wardian Workbench and cmux surface protocol are explicit compositional shells.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workbench.md), [source 2](https://www.onorca.dev/docs/model/tabs-panes-splits), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/first-workspace), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Tabs panes and splits

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - persistent tabs/panes, split Workbench, surface tabs, and context-preserving layout |
| Orca ADE | [YES] - tabs, nested right/bottom splits, persistent pane boundaries per worktree |
| cmux | [YES] - vertical/horizontal tabs, splits, workspace groups, and BonsplitView |
| Conductor | [PARTIAL] - panels and right-panel tabs; split-terminal layout not a primary documented model |
| T3 Code | [PARTIAL] - thread/sidebar and right-panel review tabs; arbitrary pane grid not a core documented feature |

**Comparison note:** cmux and Orca make terminal/surface tiling central; Wardian makes multi-tool Workbench composition central.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workbench.md), [source 2](https://www.onorca.dev/docs/model/tabs-panes-splits), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/reference/diff-viewer), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Integrated terminal

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - provider PTYs, user terminal, terminal cards, scrollback, resize, ownership, remote handoff |
| Orca ADE | [YES] - xterm.js terminal, tabs/splits, search, links, floating terminal, Quick Commands |
| cmux | [YES] - libghostty terminal with native fidelity, tabs/splits, SSH, scrollback |
| Conductor | [YES] - terminal per workspace/agent and Big Terminal Mode |
| T3 Code | [YES] - terminal sessions and PTYs owned by server; web/desktop/mobile access |

**Comparison note:** Terminal is common to all five; cmux and Wardian emphasize terminal fidelity/ownership, T3 emphasizes server ownership.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/grid.md), [source 2](https://www.onorca.dev/docs/terminal), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/reference/big-terminal-mode), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/architecture/terminal-renderers.md)

#### Native terminal fidelity and raw interactive TUI

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - ConPTY/Unix PTY, raw keys, approvals, TUIs, alternate screen, clickable links |
| Orca ADE | [YES] - Ghostty-class WebGL terminal, kitty keyboard protocol, OSC52 |
| cmux | [YES] - libghostty native terminal fidelity |
| Conductor | [PARTIAL] - terminal supports arbitrary agents/commands; renderer internals not deeply documented |
| T3 Code | [PARTIAL] - terminal renderers and provider PTY support; details are implementation-specific |

**Comparison note:** TUI/raw-key fidelity is explicitly documented for Wardian, Orca, and cmux.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/grid.md), [source 2](https://www.onorca.dev/docs/terminal), [source 3](https://cmux.com/), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/architecture/terminal-renderers.md)

#### Terminal ownership and mirrored presentations

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Owner/Mirror/Connecting, explicit focus claim, read-only mirrors, geometry broker |
| Orca ADE | [PARTIAL] - persistent panes and session restore; explicit owner/mirror model not evidenced |
| cmux | [PARTIAL] - surface focus and workspace pane control; explicit mirror semantics not a headline feature |
| Conductor | [NOT EVIDENCED]  |
| T3 Code | [NOT EVIDENCED] as a public owner/mirror terminal presentation model |

**Comparison note:** This is a Wardian-specific multi-renderer correctness feature.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workbench.md)

#### File explorer and filesystem actions

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Explorer, create/rename/delete/move, live file views, recovery, drag/drop, agent context |
| Orca ADE | [YES] - live file explorer, create/rename/delete/move, git status, remote download, find in folder |
| cmux | [YES] - file preview, open files/directories/URLs, drag/drop copy/upload |
| Conductor | [PARTIAL] - attached files/context and workspace filesystem; rich explorer not a headline surface |
| T3 Code | [YES] - filesystem RPC, projects, file browsing, uploads, and attachments |

**Comparison note:** Orca has the most explicit file-explorer interaction; T3's server owns filesystem access.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/explorer.md), [source 2](https://www.onorca.dev/docs/editing/file-explorer), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/first-workspace), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Code editor with autosave

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - file editor and source-control context; Monaco-specific experience not evidenced |
| Orca ADE | [YES] - Monaco editor, autosave, multi-cursor, HEAD-versus-working-tree view, minimap |
| cmux | [NOT EVIDENCED] as a full code editor |
| Conductor | [PARTIAL] - review/diff/workspace editing; dedicated Monaco editor not evidenced |
| T3 Code | [PARTIAL] - file editing/filesystem supported; dedicated code-editor surface not clearly documented |

**Comparison note:** Orca's Monaco editor is a documented differentiator.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workbench.md), [source 2](https://www.onorca.dev/docs/editing/monaco), [source 3](https://www.conductor.build/docs/reference/diff-viewer), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Rich Markdown editor/viewer

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Markdown library entries, editor, previews, workflow files, inspectable docs |
| Orca ADE | [YES] - rich Markdown with raw toggle, slash menu, Mermaid, tables, frontmatter, wiki links |
| cmux | [YES] - Markdown viewer surface |
| Conductor | [PARTIAL] - Markdown prompts/context and review text; rich editor not evidenced |
| T3 Code | [PARTIAL] - Markdown messages/docs; standalone rich Markdown viewer not evidenced |

**Comparison note:** Orca and cmux explicitly document Markdown surfaces; Wardian uses Markdown as a core state format.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/library.md), [source 2](https://www.onorca.dev/docs/editing/markdown), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/reference/slash-commands), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### File previews for images PDF HTML Mermaid

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - artifacts and file previews; exact format matrix is less central |
| Orca ADE | [YES] - HTML sandboxed viewer, Mermaid, PDF, image, rich Markdown viewers |
| cmux | [YES] - filePreview/markdown surfaces and open routing |
| Conductor | [PARTIAL] - attached screenshots/logs/specs and workspace review; broad viewer matrix not evidenced |
| T3 Code | [PARTIAL] - file attachments and server filesystem; rich preview matrix not evidenced |

**Comparison note:** Orca documents the broadest explicit preview matrix.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/explorer.md), [source 2](https://www.onorca.dev/docs/editing/viewers), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/first-workspace), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/composer.md)

#### Embedded browser per project or worktree

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - Browser surface exists; provider/browser integration is more limited than Orca/cmux |
| Orca ADE | [YES] - real Chromium per worktree with tabs, history, cookies, and profiles |
| cmux | [YES] - real browser pane with state persistence |
| Conductor | [NOT EVIDENCED] in reviewed first-party docs |
| T3 Code | [PARTIAL] - desktop-only Electron Chromium preview with per-thread tab state, persistent partitions, and device viewport controls; not a full Orca/cmux-style browser workspace |

**Comparison note:** Browser is a strong Orca/cmux feature; T3 has a product preview browser but not the same browser-automation workspace model.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/browser-surface.md), [source 2](https://www.onorca.dev/docs/browser/overview), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs), [source 5](https://github.com/pingdotgg/t3code/blob/main/packages/contracts/src/preview.ts)

#### Browser automation actions

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - browser surface/automation is documented but scope is smaller |
| Orca ADE | [YES] - real Chromium design and browser-use automation |
| cmux | [YES] - snapshot, eval, wait, click, fill, press, screenshot, console, network, and more |
| Conductor | [NOT EVIDENCED]  |
| T3 Code | [PARTIAL] - preview automation contracts support open/navigate/click/type/press/scroll/evaluate/wait, screenshots, recording, viewport, and color-scheme operations through a desktop host |

**Comparison note:** cmux and Orca expose broader general-purpose browser automation APIs; T3's automation is tied to its preview surface.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/browser-surface.md), [source 2](https://www.onorca.dev/docs/browser/design-mode), [source 3](https://cmux.com/docs/browser-automation), [source 4](https://github.com/pingdotgg/t3code/blob/main/packages/contracts/src/previewAutomation.ts)

#### Browser profiles cookies and identity

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - browser state/auth handling exists; product-level profile matrix not evidenced |
| Orca ADE | [YES] - per-worktree profiles with cookies, UA, viewport, Chrome/Edge import |
| cmux | [YES] - browser profiles/import, cookies, storage, and state controls |
| Conductor | [NOT EVIDENCED]  |
| T3 Code | [PARTIAL] - per-thread persistent Chromium partitions with scoped cookie/cache clearing; no documented Chrome/Edge import or profile picker |

**Comparison note:** Orca and cmux provide the clearest documented browser identity controls.
**Sources:** [source 1](https://www.onorca.dev/docs/browser/profiles), [source 2](https://cmux.com/docs/browser-automation), [source 3](https://github.com/wardian-app/Wardian/blob/main/docs/developer/browser-surface.md), [source 4](https://github.com/pingdotgg/t3code/blob/main/apps/desktop/src/preview/BrowserSession.ts)

#### Design Mode UI element capture

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] as a Wardian feature |
| Orca ADE | [YES] - click real Chromium UI element and capture HTML, computed CSS, screenshot, source file/line into prompt |
| cmux | [NOT EVIDENCED] as a named feature |
| Conductor | [NOT EVIDENCED]  |
| T3 Code | [NOT EVIDENCED]  |

**Comparison note:** Orca Design Mode is a distinctive feature.
**Sources:** [source 1](https://www.onorca.dev/docs/browser/design-mode)

#### Mobile companion or remote control UI

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - paired phone PWA with roster, agent detail, terminal/chat, lifecycle actions, Inbox, and settings limits |
| Orca ADE | [YES] - iOS/Android companion to monitor, steer, receive notifications, and follow up |
| cmux | [YES] - iOS app beta pairing |
| Conductor | [PARTIAL] - mobile client/collaboration is advertised; public availability and exact surface scope are evolving |
| T3 Code | [PARTIAL] - mobile client exists in the repository and remote-access docs describe mobile use, but the current mobile README marks distribution as in development |

**Comparison note:** All five have some mobile story; the matrix preserves current availability caveats rather than treating every mobile code path as generally shipped.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/remote-control.md), [source 2](https://www.onorca.dev/docs/mobile), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/remote-access.md), [source 6](https://github.com/pingdotgg/t3code/blob/main/apps/mobile/README.md)

#### Command palette quick open and jump navigation

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Command surface, command palette, quick actions, agent/workflow/library navigation |
| Orca ADE | [YES] - Cmd-P file search, new-tab omnibox, Cmd-J jump across worktrees/tabs |
| cmux | [YES] - command palette, custom commands, quick open |
| Conductor | [PARTIAL] - Spotlight/testing/action menus and project navigation; exact quick-open surface not evidenced |
| T3 Code | [YES] - Cmd/Ctrl+K Command Palette, Add Project, settings/actions, thread navigation |

**Comparison note:** Navigation is a common shell feature, but implementations differ.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/command-panel.md), [source 2](https://www.onorca.dev/docs/model/quick-open), [source 3](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md), [source 4](https://www.conductor.build/docs/concepts/testing), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/source-control.md)

#### Adaptive agent overview layout modes

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Agents Overview supports Auto, Grid, and Single/focused layouts with responsive sizing and terminal/chat cards |
| Orca ADE | [YES] - Agent Dashboard kanban with status/project/PR filters and pop-out; layout model differs |
| cmux | [NOT EVIDENCED] as a multi-agent overview layout surface |
| Conductor | [PARTIAL] - workspace/home and agent views; overview layout modes not richly documented |
| T3 Code | [PARTIAL] - thread/sidebar and mobile navigation; no equivalent Agent Overview layout modes evidenced |

**Comparison note:** This compares fleet-monitoring presentation, not terminal splits.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/agents-overview.md), [source 2](https://www.onorca.dev/docs/model/agents-sessions), [source 3](https://www.conductor.build/docs/cloud/collaboration), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/thread-sidebar.md)

#### Live dashboard fleet table and trend metrics

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Dashboard table by agent with state, trend, active time, turns, tokens, files, lines, provider strip, and selectable window |
| Orca ADE | [PARTIAL] - usage/rate-limit dashboard and agent dashboard; exact fleet metric table not evidenced |
| cmux | [NOT EVIDENCED] as a provider/model fleet dashboard |
| Conductor | [PARTIAL] - workspace status/checks/resource status; exact fleet trend table not evidenced |
| T3 Code | [PARTIAL] - usage page and diagnostics/resource views; exact Wardian dashboard table not evidenced |

**Comparison note:** Wardian's Dashboard and Analytics split live fleet monitoring from historical analysis.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/dashboard.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/guide/analytics.md), [source 3](https://www.onorca.dev/docs/agents/usage-tracking), [source 4](https://www.conductor.build/docs/reference/checks), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/user/usage.md)

#### Standalone user terminal distinct from agent terminals

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - separate user terminal surface alongside managed provider terminals |
| Orca ADE | [YES] - floating terminal and terminal tabs independent of agent sessions |
| cmux | [YES] - terminal is the primary primitive and can launch arbitrary commands |
| Conductor | [YES] - Big Terminal Mode and ordinary terminal commands alongside agent work |
| T3 Code | [YES] - server-owned terminal sessions distinct from provider threads |

**Comparison note:** This separates operator shell work from provider-owned agent sessions.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workbench.md), [source 2](https://www.onorca.dev/docs/terminal), [source 3](https://cmux.com/), [source 4](https://www.conductor.build/docs/reference/big-terminal-mode), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/architecture/terminal-renderers.md)

#### Guarded file editor and recovery workflow

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Files surface has read/source modes, explicit saves, root/capability checks, file watcher updates, recovery records, and bounded previews |
| Orca ADE | [PARTIAL] - Monaco editor/autosave and file explorer; guarded recovery contract not evidenced |
| cmux | [PARTIAL] - file preview/open and drag-drop; no comparable guarded editor workflow evidenced |
| Conductor | [PARTIAL] - workspace files/context/review; dedicated recovery/editor contract not evidenced |
| T3 Code | [PARTIAL] - server filesystem and file attachments; editor/recovery semantics not richly documented |

**Comparison note:** Wardian prioritizes filesystem authority and recoverability as first-class behavior.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/explorer.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/developer/workbench-surfaces.md), [source 3](https://www.onorca.dev/docs/editing/monaco), [source 4](https://www.conductor.build/docs/first-workspace), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/remote.md)

#### Artifact review and evidence snapshots

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - artifact/file review surfaces, agent change snapshots, provenance, and workflow evidence are inspectable and triageable |
| Orca ADE | [YES] - artifacts, diff/review, rich repo previews, and worktree checkpoints |
| cmux | [PARTIAL] - cmux-artifact skill, screenshots, files, and logs; no equivalent review surface evidenced |
| Conductor | [PARTIAL] - attached specs/screenshots/logs, diff/review, and workspace artifacts |
| T3 Code | [YES] - work artifacts, checkpoint diffs, turn summaries, and review surfaces |

**Comparison note:** Artifact is used differently: Wardian emphasizes evidence lineage; Orca/T3 emphasize worktree/turn review.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/specs/2026-07-16-files-artifact-review-surface.md), [source 2](https://github.com/stablyai/orca), [source 3](https://github.com/manaflow-ai/cmux-skills), [source 4](https://www.conductor.build/docs/first-workspace), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/work-artifacts.md)

#### Custom programmable sidebar UI

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] in reviewed first-party Wardian docs |
| Orca ADE | [NOT EVIDENCED] in reviewed first-party Orca docs |
| cmux | [YES] - beta interpreted SwiftUI-style or JSON custom sidebars, live workspace data, hot reload, actions, and in-process/remote rendering |
| Conductor | [NOT EVIDENCED] in reviewed first-party docs |
| T3 Code | [NOT EVIDENCED] in reviewed first-party docs |

**Comparison note:** cmux exposes a user-programmable native sidebar surface beyond fixed product UI.
**Sources:** [source 1](https://github.com/manaflow-ai/cmux/blob/main/docs/custom-sidebars.md), [source 2](https://github.com/manaflow-ai/cmux/blob/main/docs/cli-contract.md)

#### cmux AI workspace context assistant

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED]  |
| Orca ADE | [NOT EVIDENCED]  |
| cmux | [EXPERIMENTAL] - Founder's Edition early access describes cmux AI that gives context on every workspace, tab, and panel |
| Conductor | [NOT EVIDENCED]  |
| T3 Code | [NOT EVIDENCED]  |

**Comparison note:** This is labeled early access rather than a generally shipped capability.
**Sources:** [source 1](https://github.com/manaflow-ai/cmux#founders-edition)

#### Voice mode for terminal/workspace interaction

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED]  |
| Orca ADE | [NOT EVIDENCED]  |
| cmux | [EXPERIMENTAL] - Founder's Edition early access lists Voice mode |
| Conductor | [NOT EVIDENCED]  |
| T3 Code | [NOT EVIDENCED]  |

**Comparison note:** This is a cmux early-access feature, not the same as provider voice support.
**Sources:** [source 1](https://github.com/manaflow-ai/cmux#founders-edition)

#### Desktop Computer Use through accessibility tree and screenshots

| Product | Assessment |
| --- | --- |
| Wardian | [NOT EVIDENCED] in reviewed first-party Wardian docs |
| Orca ADE | [YES] - beta native desktop computer-use actions use accessibility tree/screenshots with click/set/type/scroll and a permission gate |
| cmux | [NOT EVIDENCED] in reviewed first-party sources |
| Conductor | [NOT EVIDENCED] in reviewed first-party sources |
| T3 Code | [NOT EVIDENCED] in reviewed first-party sources |

**Comparison note:** Orca's Computer Use is separate from its browser automation and is a distinctive capability in this comparison.
**Sources:** [source 1](https://www.onorca.dev/docs/cli/computer-use)

#### Browser device viewport emulation and color scheme

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - browser surface resizes to pane and supports screenshot/viewport operations |
| Orca ADE | [YES] - browser profiles include UA/viewport and design-mode capture |
| cmux | [YES] - browser automation and viewport/devtools controls |
| Conductor | [NOT EVIDENCED] in reviewed first-party docs |
| T3 Code | [YES] - preview device presets, freeform CSS dimensions, rotation/aspect controls, zoom, and light/dark/system color-scheme emulation |

**Comparison note:** T3 has browser preview/device controls even though it does not expose Orca/cmux's general browser workspace.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/browser.md), [source 2](https://www.onorca.dev/docs/browser/profiles), [source 3](https://cmux.com/docs/browser-automation), [source 4](https://github.com/pingdotgg/t3code/blob/main/packages/contracts/src/previewAutomation.ts)

### Workflows

#### Visual workflow builder

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - visual node graph builder, node settings, variable assistant, validation, run/observe/monitor modes |
| Orca ADE | [PARTIAL] - orchestration Run/Tasks/Dispatches and decision gates; visual builder not evidenced |
| cmux | [NOT EVIDENCED] as a workflow builder |
| Conductor | [PARTIAL] - workflow concepts and run scripts; no visual graph builder evidenced |
| T3 Code | [PARTIAL] - orchestration layers and checkpoints; no end-user visual builder evidenced |

**Comparison note:** Wardian is the only product in this set with a documented visual workflow view.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workflows.md), [source 2](https://www.onorca.dev/docs/cli/orchestration), [source 3](https://www.conductor.build/docs/concepts/workflow), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### DAG tasks dependencies and dispatches

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - deterministic workflow engine, assignments, branches, loops, waits, shared storage, and run state |
| Orca ADE | [YES] - structured Run/Tasks/Dispatches, supervised workers, dependencies, heartbeat/completion, decision gates |
| cmux | [NOT EVIDENCED] as a durable orchestration DAG |
| Conductor | [PARTIAL] - independent workspace units and parallel-agent workflow; no public DAG primitive evidenced |
| T3 Code | [YES] - orchestration commands, reactors, projections, checkpoints, and thread turns; public DAG task UX not evidenced |

**Comparison note:** Orca and T3 have orchestration layers; Wardian exposes a user-facing deterministic workflow engine.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/workflow-engine.md), [source 2](https://www.onorca.dev/docs/cli/orchestration), [source 3](https://www.conductor.build/docs/concepts/workflow), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Branch loop and wait control flow

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - deterministic branch/loop/wait nodes with workflow-owned execution |
| Orca ADE | [PARTIAL] - decision gates and dependencies; loop semantics not evidenced |
| cmux | [NOT EVIDENCED]  |
| Conductor | [PARTIAL] - workflow concepts mention independent work and verification; control-flow nodes not evidenced |
| T3 Code | [NOT EVIDENCED] as an end-user workflow control-flow surface |

**Comparison note:** This is a deep workflow-engine comparison; absent means no first-party evidence found.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/workflow-engine.md), [source 2](https://www.onorca.dev/docs/cli/orchestration), [source 3](https://www.conductor.build/docs/concepts/workflow), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Manual scheduled and listener triggers

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - manual, scheduled, and listener triggers plus persisted schedules |
| Orca ADE | [YES] - scheduled automations with hourly/daily/weekdays/weekly/cron/RRULE |
| cmux | [NOT EVIDENCED] as scheduled workflows |
| Conductor | [PARTIAL] - workflow/run scripts and project automation; scheduled triggers not central |
| T3 Code | [PARTIAL] - background service and server commands; scheduled workflow trigger not evidenced |

**Comparison note:** Orca has a strong documented automation scheduler; Wardian combines schedule and listener triggers.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workflows.md), [source 2](https://www.onorca.dev/docs/cli/automations), [source 3](https://www.conductor.build/docs/concepts/workflow), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/user/background-service.md)

#### Workflow parameters and shared storage

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - input schemas/parameters, variables, shared workflow storage, provider/workspace assignment |
| Orca ADE | [PARTIAL] - durable orchestration namespace/inbox and worker state |
| cmux | [NOT EVIDENCED]  |
| Conductor | [PARTIAL] - attached context and environment variables; workflow parameter schema not evidenced |
| T3 Code | [PARTIAL] - thread composer attachments and orchestration commands; public workflow storage not evidenced |

**Comparison note:** Wardian's workflow runtime is the most explicit application-level shared state model.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/developer/workflow-engine.md), [source 2](https://www.onorca.dev/docs/cli/orchestration), [source 3](https://www.conductor.build/docs/cloud/environment-variables), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Run monitoring and workflow outcome triage

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - Workflows Monitor, active/recent runs, graph state, timeline, node outputs, approval controls, Inbox outcomes |
| Orca ADE | [PARTIAL] - orchestration status/heartbeat/completion and Agents feed |
| cmux | [NOT EVIDENCED]  |
| Conductor | [YES] - checks, testing, workspace status, PR/review, and collaboration |
| T3 Code | [PARTIAL] - projections/subscriptions, thread status, and diagnostics; no workflow monitor UI evidenced |

**Comparison note:** Wardian has a dedicated run observer; Conductor has workspace-level checks and testing.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workflows.md), [source 2](https://www.onorca.dev/docs/cli/orchestration), [source 3](https://www.conductor.build/docs/reference/checks), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Workflow approvals and gates

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - workflow approval nodes project to Inbox and engine remains authoritative |
| Orca ADE | [YES] - supervised workers and decision gates |
| cmux | [NOT EVIDENCED]  |
| Conductor | [PARTIAL] - agent permissions and review gates; workflow approval node not evidenced |
| T3 Code | [PARTIAL] - provider approval/user-input events and checkpoint revert; workflow approval UX not evidenced |

**Comparison note:** Wardian's workflow approval projection is distinct from provider permissions.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/inbox.md), [source 2](https://www.onorca.dev/docs/cli/orchestration), [source 3](https://www.conductor.build/docs/concepts/agent-modes), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Checkpoint capture revert replay or undo

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - workflow runs, snapshots, source-control recovery, and durable run evidence |
| Orca ADE | [YES] - worktree checkpoints with status/comments; compare/review lifecycle |
| cmux | [PARTIAL] - session restore and scrollback; no Git checkpoint model evidenced |
| Conductor | [PARTIAL] - workspace review/merge/archive; no explicit checkpoint revert evidenced |
| T3 Code | [YES] - checkpoint reactor captures turn start/completion and supports checkpoint revert; transactional event projections |

**Comparison note:** T3's checkpoint reactor and Orca's worktree checkpoints are the clearest matches.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workflows.md), [source 2](https://www.onorca.dev/docs/cli/worktree-checkpoints), [source 3](https://www.conductor.build/docs/reference/diff-viewer), [source 4](https://github.com/pingdotgg/t3code/blob/main/docs/internals/providers.md)

#### Workflow node catalog for agent and non-agent actions

| Product | Assessment |
| --- | --- |
| Wardian | [YES] - node types cover agent calls, commands, scripts, shell, files, memory, branches, loops, waits, and workflow control |
| Orca ADE | [PARTIAL] - orchestration dispatch/task primitives; no comparable public node catalog |
| cmux | [NOT EVIDENCED] as a workflow node system |
| Conductor | [PARTIAL] - scripts, tests, review, and workflow concepts; no public node catalog |
| T3 Code | [PARTIAL] - orchestration commands/reactors; no public end-user node catalog |

**Comparison note:** Wardian is the only product with a documented end-user workflow node reference in this set.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/workflows/node-reference-v2.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/developer/workflow-engine.md), [source 3](https://www.onorca.dev/docs/cli/orchestration), [source 4](https://www.conductor.build/docs/concepts/workflow), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/internals/overview.md)

#### Session-close memory consolidation automation

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - editable Memory Consolidation workflow sample and session-close scheduler exist but are optional/configured |
| Orca ADE | [NOT EVIDENCED] in reviewed first-party sources |
| cmux | [NOT EVIDENCED]  |
| Conductor | [NOT EVIDENCED]  |
| T3 Code | [NOT EVIDENCED]  |

**Comparison note:** This is an optional Wardian workflow, not the default memory behavior.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/agent-memory.md), [source 2](https://github.com/wardian-app/Wardian/blob/main/docs/workflows/samples.md)

#### Spotlight testing and per-workspace verification

| Product | Assessment |
| --- | --- |
| Wardian | [PARTIAL] - workflow shell/script execution, run monitoring, and evidence; no Spotlight-branded testing surface |
| Orca ADE | [PARTIAL] - browser/preview, terminal, and agent verification paths |
| cmux | [PARTIAL] - browser automation and terminal verification; no Spotlight surface |
| Conductor | [YES] - Spotlight testing for app/server/watcher/unit tests with per-workspace ports and workspace sync |
| T3 Code | [PARTIAL] - tests/diagnostics and project scripts; no Spotlight-branded testing surface |

**Comparison note:** Conductor names a dedicated testing workflow; the others expose underlying verification primitives.
**Sources:** [source 1](https://github.com/wardian-app/Wardian/blob/main/docs/guide/workflows.md), [source 2](https://www.onorca.dev/docs/browser/design-mode), [source 3](https://cmux.com/docs/browser-automation), [source 4](https://www.conductor.build/docs/concepts/testing), [source 5](https://github.com/pingdotgg/t3code/blob/main/docs/operations/observability.md)


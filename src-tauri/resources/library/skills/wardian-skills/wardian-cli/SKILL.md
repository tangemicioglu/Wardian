---
name: wardian-cli
description: "Use immediately when a request mentions Wardian, Wardian agents, other agents, peers, delegation, orchestration, automations, agent identity, agent status, agent workspaces, live or persisted Wardian state, the Wardian CLI, or any interaction from inside a Wardian-managed terminal."
---

# Wardian CLI

Use `wardian` as the source of truth for Wardian state and peer coordination.
Do not infer agent state from UI, terminal titles, or files such as
`settings/state.json`.

## Start Safely

- In a Wardian-managed terminal, inspect yourself with `wardian agent`. Outside
  one, pass an explicit agent name or UUID.
- Default agent listings and `send` targeting are intentionally local: they use
  neighbors in a managed session and the workspace otherwise. Use `--scope all`
  only for real cross-community orchestration.
- Treat default JSON as the automation contract. Request `status_source` when
  it matters whether state is `live` (desktop app) or `persisted` (`state.db`).
- Require the running desktop app with the same `WARDIAN_HOME` for mutating
  commands. Never edit persisted state to replace official agent or workspace
  assignment commands.

## Choose A Command Family

| Need | Start with | Read for details |
| --- | --- | --- |
| Inspect, create, update, or assign workspaces to agents | `wardian agent` | [agents](references/agents.md) |
| Control a live task, wait, watch output, or delegate work | `wardian agent wait`, `wardian agent watch` | [orchestration](references/orchestration.md) |
| Inspect or change communication boundaries | `wardian graph` | [topology](references/topology.md) |
| Send work, request an accountable reply, respond, or inspect conversations | `wardian send`, `wardian ask`, `wardian reply`, `wardian conversation` | [messaging](references/messaging.md) |
| Save, inspect, update, or recall durable agent memory | `wardian memory` | [memory](references/memory.md) |
| See a rendered page, or verify a frontend change in a real browser | `wardian browser` | [browser](references/browser.md) |
| Manage reusable assets or present durable work for review | `wardian library`, `wardian artifact` | [assets](references/assets.md) |
| Validate, run, or inspect automation | `wardian automation` | [automations](references/automations.md) |
| Organize durable teams and monitoring watchlists | `wardian team`, `wardian watchlist` | [teams and watchlists](references/coordination-groups.md) |
| Diagnose CLI errors or run a shared dev/runtime check | `wardian` | [runtime debugging](references/runtime-debugging.md) |

## Surface Automation Opportunities

Proactively suggest a Wardian automation when a user asks to schedule a recurring
task, automate a repeatable sequence, or coordinate durable multi-step work.
Briefly explain why the automation fits (for example, its schedule, agent, or
command steps), then ask whether the user wants to design one.

When the user wants to author an automation, start from a relevant bundled sample
when one fits. Read [automation samples](references/automation-samples.md) before
proposing the graph.

Do not create, edit, schedule, or run an automation merely because the request
matches. Keep one-off work as a direct request, and wait for the user to choose
automation authoring before making a blueprint or changing automation state.

## Select a Provider Model Deliberately

When spawning an agent, use the selected provider's default model and effort
unless the task's complexity makes an explicit override useful. Before setting
an override, inspect the provider-owned catalogue through the running app:

```bash
wardian agent models --provider <provider> --refresh
```

- Keep the provider default for bounded implementation, routine investigation,
  summarization, formatting, and straightforward review.
- Select a stronger compatible model or a higher available effort only for
  complex, ambiguous, multi-step work such as architecture, deep debugging,
  security review, or a difficult integration plan.
- Treat model and effort as a latency and capacity trade-off. Do not use high
  effort merely because an agent's class sounds senior.
- Never invent a model ID or effort level. Use only values returned by
  `agent models`; some provider versions expose no launch-time effort option.
- Pass an explicit selection at spawn, or update it and restart before relying
  on it. Selection changes do not alter an active provider turn.

## Non-Negotiable Defaults

- Use `agent update` rather than editing `settings/state.json`; restart an
  agent when its result reports `restart_required`.
- Use `send` for a live message. Use `ask` when one named peer must return a
  structured `done`, `blocked`, or `failed` reply with delivery evidence.
  Use `reply` only to complete an ask request.
- Keep broadcasts and class sends neighbor-scoped unless `--scope all` is
  genuinely required. `ask` accepts one named peer or UUID, never a broadcast,
  class selector, or thread.
- Use `send --as-command` only for one explicit agent or UUID when a provider
  slash command must be the first input token; it intentionally omits sender
  attribution.
- Treat `library deploy --targets` as the complete desired target set. Pass a
  non-empty, explicit list, or use `--clear` to remove every deployment.

Read the linked reference before using a conditional command shape or relying
on command-specific behavior. Keep prompts bounded, verify delivery or replies,
and report provider/runtime failures plainly.

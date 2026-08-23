# Pi Coding Agent Provider

- **Status:** Implemented
- **Date:** 2026-08-23
- **Issue:** #943
- **Upstream:** [Pi coding agent documentation](https://pi.dev/docs/latest/)

## Context

Pi is an extensible coding-agent CLI distributed as
`@earendil-works/pi-coding-agent`. Wardian needs a provider adapter that keeps
Pi's native terminal, project context, packages, extensions, and model
configuration while providing Wardian-owned session isolation, telemetry,
headless workflow execution, and transcript replay.

Pi has three relevant modes:

- interactive TUI, with `regular` and `fullscreen` rendering;
- non-interactive JSON events through `--mode json`;
- bidirectional JSON-RPC through `--mode rpc`.

Pi also persists a versioned JSONL session tree. A caller can assign an exact
project session ID with `--session-id`, resume it with `--session`, and select a
separate storage root with `--session-dir`. This lets Wardian create a provider
session without a hidden model bootstrap turn.

## Capability Map

| Wardian responsibility | Pi capability | Selected mapping |
| --- | --- | --- |
| Visible provider terminal | Interactive TUI | `pi --tui-mode regular` so xterm keeps native scrollback |
| Exact fresh identity | `--session-id <id>` | Generate a UUID distinct from the Wardian agent UUID |
| Exact resume | `--session <id-or-path>` | Resume only the persisted provider ID bound to the agent |
| Session isolation | `--session-dir <path>` | Store JSONL under `<absolute-wardian-home-path>/agents/<agent-id>/pi/sessions` |
| Project instructions | Parent/project `AGENTS.md`; `--append-system-prompt` | Keep native project discovery and append each Wardian-managed `AGENTS.md` file |
| Agent skills | Agent Skills discovery; repeated `--skill` | Pass each Wardian-managed `.agents/skills` directory explicitly |
| Model selection | `--model <provider/model>`; `--list-models` | Discover the installed catalogue and pass the exact selected ID |
| Reasoning level | `--thinking off|minimal|low|medium|high|xhigh|max` | Store as `reasoning_effort` in Pi's typed provider config |
| Tool selection | `--tools`, `--exclude-tools`, `--no-tools` | Expose typed advanced settings and preserve custom args |
| Project trust | `--approve`, `--no-approve` | Optional one-launch override, labeled as project-local configuration trust |
| Offline startup | `--offline` | Optional advanced setting; does not disable model API calls needed for a turn |
| Headless workflows | `--mode json <prompt>` | Parse session, message, tool, and agent completion events |
| Status and watch output | Session JSONL records | Tail the exact session file for user, generation, tool, and completion events |
| Chat replay | Version 3 JSONL messages | Normalize user, assistant, tool-call, and tool-result records |
| Multiline delivery | Bracketed-paste-aware editor | Bracket prompts, wait for Pi's editor settle, then send carriage return |
| Rich remote control | JSON-RPC mode | Supported by Pi but deferred; replacing the native TUI is not required here |
| Custom lifecycle hooks | Extensions | Supported by Pi but not injected by Wardian; JSONL is sufficient and less invasive |

## Decision

Add `pi` as a first-class provider ID and `Pi` as its user-facing label.

Visible agents run in the real target workspace. Wardian does not replace Pi's
global agent directory, so existing authentication, settings, themes, packages,
and extensions continue to work. Only provider session JSONL is redirected to
the Wardian agent directory.

Fresh visible launch:

```bash
pi --tui-mode regular \
  --session-dir <absolute-wardian-home-path>/agents/<agent-id>/pi/sessions \
  --session-id <provider-session-id> \
  --name <agent-name>
```

PowerShell:

```powershell
pi --tui-mode regular `
  --session-dir <absolute-wardian-home-path>\agents\<agent-id>\pi\sessions `
  --session-id <provider-session-id> `
  --name <agent-name>
```

Headless workflow launch:

```bash
pi --session-dir <wardian-session-dir> --session-id <provider-session-id> \
  --mode json "<prompt>"
```

For resume, Wardian replaces `--session-id` with `--session
<provider-session-id>`. Wardian never uses Pi's recent-session selector or a
newest-file fallback.

Pi creates the JSONL file only after the first persisted entry. The watcher
therefore waits for a file whose session header contains the exact expected
provider ID. A matching header confirms identity, promotes the fresh ID to the
agent's resume ID, and becomes the source for lifecycle and Chat events.

## Security Boundary

Pi's project trust protects whether project-local settings, extensions, and
`.agents` skills load. It is not a process sandbox or a command approval system.
Pi's built-in shell tool and extensions run with the permissions of the Wardian
desktop process. Wardian must not present `--approve` as autonomous mode,
sandboxing, or per-command approval.

Use external OS, VM, or container isolation for unattended work on untrusted
repositories. Pi also requires a Bash-compatible shell on Windows; Git Bash is
the standard supported setup.

## Consequences

- Pi keeps its own terminal UX and ecosystem instead of receiving a
  Wardian-specific extension dependency.
- Wardian-managed role instructions and skills remain inspectable on disk and
  do not need to be copied into the target repository.
- Fresh and resumed provider identity are deterministic and cannot attach to a
  different Pi project session.
- Headless and interactive paths share model, thinking, context, skill, trust,
  tool, offline, and custom-argument settings.
- RPC remains an option for a future non-terminal integration, but it is not
  used to emulate the visible provider surface.
- Real-provider validation is opt-in because model access depends on the user's
  Pi provider configuration. A local OpenAI-compatible model endpoint can be
  used for deterministic validation without a hosted subscription.

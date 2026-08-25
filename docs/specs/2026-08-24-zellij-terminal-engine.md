# Zellij Terminal Engine Replacement

Date: 2026-08-24

## Purpose

Wardian's terminal runtime has accumulated provider-specific repaint,
scrollback, renderer-lifecycle, ownership, and recovery logic around one PTY
and one or more xterm.js renderers per agent. The result is expensive at
Habitat scale and repeatedly fragile across ConPTY, xterm.js, foreground
recovery, and TUI redraw behavior.

Zellij 0.45.0 replaces that runtime. Zellij owns provider PTYs, pane layout,
scrollback, and terminal emulation. Wardian remains the control plane for agent
identity, lifecycle, secure launch configuration, structured delivery,
telemetry, and Workbench presentation.

This is a replacement, not an optional terminal provider. The branch is not
merge-ready while the former per-agent PTY or many-xterm architecture remains
as a production fallback.

## Decisions

- Pin and bundle the Zellij 0.45.0 `no-web` executable for Windows x64,
  Linux x64 and arm64, and macOS x64 and arm64.
- Run one Zellij session per Wardian home. Its name is derived from the
  canonical Wardian-home path so production, development, and isolated test
  homes cannot attach to one another.
- Keep one attached Zellij client alive for the session. On Windows, launch it
  in a hidden native console and persist its PID. On Linux and macOS, attach it
  through one Wardian-owned PTY. A background server without a live attached
  client is not a running engine.
- Run one provider in one Zellij terminal pane. Wardian persists the mapping
  from agent session ID to Zellij pane ID and runtime generation.
- Use Zellij's documented CLI actions and JSON output as the integration
  boundary. Wardian does not depend on Zellij's private socket or web-client
  protocols.
- Keep remote terminal transport out of the first desktop/native release, but
  retain agent-addressed backend operations so a later authenticated remote
  presentation can attach without changing provider ownership.
- Do not enable Zellij session resurrection. Wardian owns durable agent intent
  and performs explicit reconciliation; it must never replay provider commands
  from Zellij metadata after a reboot.

## Bundled runtime

The staging script downloads an exact release asset only when the selected
binary is absent from the build resources. It verifies the extracted
executable against the upstream SHA-256 digest before staging it. The archive
checksum is not used because Zellij's published checksum files cover the
executable inside each archive.

The build uses the smaller `no-web` artifacts because Wardian does not expose
Zellij's HTTP server or private web protocol. A developer override may point to
another Zellij executable for tests, but production builds always stage and
verify 0.45.0.

Zellij configuration, layouts, and launch files live under the active
Wardian home. Sessions use Zellij's per-user socket root because its Windows
client does not reliably preserve a custom socket-directory environment through
a nested ConPTY. The Wardian-home hash in every session name keeps production,
development, and isolated tests separate. Wardian does not read the user's
global Zellij configuration.

## Runtime topology

```text
Wardian AppState
  |
  +-- ZellijTerminalEngine (one per Wardian home)
       |
       +-- attached client ----------> Zellij session/server
       |                                  |
       |                                  +-- pane: agent A -> provider A
       |                                  +-- pane: agent B -> provider B
       |                                  +-- pane: agent C -> provider C
       |
       +-- pane-addressed CLI actions
       |     new-pane, write, focus-pane-id, dump-screen, list-panes,
       |     subscribe, close-pane
       |
       +-- generation-scoped pane registry
             agent ID, pane ID, generation, launch identity
```

The attached client keeps the Zellij server and provider panes alive. The
desktop does not render that client on Windows. Each provider pane has one JSON
snapshot subscription, and exactly one app-level xterm.js host renders the
selected agent's broker stream. Agent cards that do not own that host show
read-only broker snapshots and consume no xterm.js or WebGL instance.
Activating a card focuses and fullscreens its pane before the singleton host
moves there.
At most one xterm.js renderer and one WebGL context are live for agent
terminals, regardless of agent count.

The standalone human terminal is a separate product surface and is not part of
this replacement.

## Secure provider launch

Zellij pane processes inherit the long-lived server environment, not the
environment of the CLI action that creates a pane. Wardian therefore cannot
pass per-agent values such as `WARDIAN_SESSION_ID`, `CODEX_HOME`, provider
configuration, or a memory capability through the Zellij process environment.
It also must not place those values on a command line.

Wardian writes one versioned, nonce-bound JSON manifest beneath
`WARDIAN_HOME/runtime/zellij/launches/`. The file contains the executable,
exact argument vector, working directory, and environment overlay. Wardian
places only the unpredictable launch-file path on the pane command line.

The bundled CLI validates and deletes the manifest before starting the
provider. On Linux and macOS it replaces itself with the provider by using
`exec`. On Windows it starts the provider with inherited ConPTY handles and
waits for its exit.

Wardian removes abandoned `.json` launch files during engine startup. A launch
file is not a durable authorization record. Memory authority
remains scoped to the in-memory capability lease for that provider generation.

## State model

### Engine state

| State | Meaning | Allowed transition |
|---|---|---|
| `stopped` | No attached client is tracked by Wardian | `starting` |
| `starting` | Configuration exists and an attached client is being created | `running`, `failed` |
| `running` | Attached client is live and CLI health succeeds | `reattaching`, `stopped` |
| `reattaching` | Client died; the next activation or spawn creates a replacement | `running`, `failed` |
| `failed` | A bounded start/recovery attempt failed | `starting`, `stopped` |

Only one transition mutates engine state at a time. No process wait, CLI call,
or filesystem operation occurs while the engine mutex is held.

### Agent pane state

| State | Meaning | Allowed transition |
|---|---|---|
| `unbound` | No live pane is associated with the agent | `starting` |
| `starting` | One-use launch file exists and `new-pane` is in flight | `running`, `unbound` |
| `running` | Pane identity and current generation are registered | `exited`, `closing` |
| `exited` | Zellij reports the command exited or the pane closed | `starting`, `unbound` |
| `closing` | Wardian requested pane closure | `unbound` |

Pane IDs are generation-scoped. A delayed subscription, input, focus, close,
or status event from an older generation is rejected.

## Lifecycle traces

### Cold start

1. Resolve and verify the bundled executable.
2. Create isolated Zellij configuration and socket directories.
3. Remove abandoned one-use launch files.
4. Start the attached client with a neutral control pane.
5. Wait for the named session to answer `list-panes`.
6. Start each configured live agent once through the existing restore path.
7. Close the neutral control pane after the first provider pane is running.

### Agent spawn

1. Build the provider launch specification and per-agent environment exactly
   as Wardian does for headless provider validation.
2. Issue a memory capability for this provider generation.
3. Write and sync the one-use manifest.
4. Run `new-pane --name <stable Wardian label>` with the terminal host.
5. Parse the returned `terminal_<id>`, or reconcile it by stable pane title,
   and register it atomically.
6. Start a pane subscription and provider-native log watchers.
7. Mark the generation ready only after provider-native evidence or the current
   rendered pane proves a usable prompt.

If any step fails, remove the launch file, close a newly created pane when its ID
is known, revoke the memory capability, and return the agent to `unbound`.

### Input and delivery

- Human input goes through the active agent's broker runtime. The runtime uses
  a generation-checked, pane-addressed Zellij `write` action.
- Structured Chat, Inbox, workflow, CLI, and broadcast delivery uses
  pane-addressed `write` actions under the existing per-agent delivery lock.
- A delivery generation includes both the Wardian provider-input generation
  and the Zellij pane generation. Stale work cannot write to a replacement
  pane.
- CLI action calls for one pane are serialized. Different panes may be driven
  concurrently.

### Focus and fitting

1. Verify the target pane belongs to the requested agent and generation.
2. Focus the pane by ID and make it fullscreen.
3. Move the singleton terminal host to the selected presentation.
4. Apply the selected pane's next complete snapshot to that host.

Snapshot cards never resize panes. The singleton xterm fits the canonical
Zellij frame locally. Remote clients cannot activate or resize this desktop
session in the first release.

### Renderer or Workbench restart

Zellij and the backend subscriptions remain alive. The singleton renderer
registers again and replaces its local state from the selected agent's broker
snapshot. Provider and pane generations do not change.

Wardian does not authorize an unregistered pane after a backend process
restart. The app-lifetime Windows Job Object terminates the old process tree.
Wardian then starts configured live agents with new pane and memory-capability
generations.

### Attached client loss

Pane subscriptions and broker runtimes remain usable while the server answers.
The next terminal activation or agent spawn starts a new attached client and
retains provider and pane generations.

### Zellij server loss

Pane subscriptions end and the affected agents enter the existing exited
terminal state. **Start Session** or **Restart Session** closes the stale
generation, starts one new attached session, and creates a new pane under the
agent lifecycle lock. Wardian never uses Zellij resurrection or a stale launch
manifest.

### Pause, clear, restart, and delete

- **Pause** closes the pane, revokes its memory capability, and retains agent
  configuration.
- **Clear/New Session** closes the old pane, advances provider and pane
  generations, clears provider identity according to existing provider rules,
  and starts one replacement pane.
- **Restart** closes and replaces the pane without changing durable provider
  identity unless the existing restart contract requires it.
- **Delete** closes the pane before removing the agent's durable records.

Closing a Workbench surface never closes a pane.

## Observation and telemetry

Zellij `subscribe --format json --ansi` emits complete rendered pane viewports,
not provider PTY byte streams. Wardian treats each update as a replacement
frame. It does not append frames, infer raw byte deltas, or reconstruct a
provider transcript from repaint differences.

- Terminal cards use replacement frames for noninteractive previews.
- Provider-native logs, hooks, and structured state remain authoritative for
  transcript events, session identity, turn receipts, and completion.
- Current-frame prompt and action-required detection may establish startup
  readiness when a provider has no native event.
- `list-panes --all --json` is authoritative for pane existence, exit status,
  title, command identity, and geometry.
- Internal Zellij errors are logged with agent and generation context; the UI
  receives stable Wardian error codes and recovery copy, not socket paths,
  commands, tokens, or raw stderr.

## Desktop UX

- Agent cards retain Terminal and Chat modes.
- A nonactive Terminal card shows a live, noninteractive pane preview and an
  explicit **Activate terminal** affordance.
- Activating a terminal moves the single live terminal presentation to that
  card or Agent Session surface and preserves keyboard focus.
- The active card identifies itself without exposing Zellij implementation
  detail in routine use.
- If the engine is recovering, previews show **Reconnecting terminal…** and no
  input affordance.
- If the pane exited, the card shows the provider exit state and the existing
  Start/Restart lifecycle action. Retry never writes to a stale pane.

## Compatibility and removal

The replacement removes these production responsibilities from Wardian:

- one `portable-pty` provider PTY per agent;
- per-agent Rust VT parsers, replay journals, and canonical screen snapshots;
- independent xterm.js renderers for every terminal presentation;
- provider-specific scrollback and repaint reconstruction;
- owner/mirror geometry arbitration between copies of one agent terminal.

The terminal-session broker remains the ordered presentation and input API for
pane subscriptions. No per-agent Wardian-owned provider PTY runtime or
many-renderer fallback remains reachable in a release build.

## Verification

Unit tests must cover:

- platform artifact selection and every pinned executable digest;
- launch-file path, nonce, cleanup, argument, environment, and redaction
  rules;
- command construction and pane-ID parsing without invoking a shell;
- every engine and pane state transition, including stale generations;
- reconciliation of matching, missing, exited, and unknown panes;
- input serialization and rejection after clear/restart;
- replacement-frame handling without duplicate transcript output;
- exactly one active terminal renderer across Agents and Agent Session
  presentations.

Windows native tests are the first required acceptance gate and must prove:

- the bundled Zellij executable passes checksum and `--version` validation;
- a real attached client keeps multiple ConPTY provider panes alive;
- pane-addressed input reaches only the selected pane;
- focus and fullscreen actions select the intended pane without changing another pane;
- pause, restart, clear, and delete do not leave provider descendants;
- a multi-agent fixture still has one agent xterm.js renderer and one WebGL
  context, with no terminal event gaps.

Frontend lint, unit tests, production build, backend clippy/test/check, browser
E2E, and native Workbench smoke must pass. A frontend PR must include a
feature-specific screenshot showing one active terminal and adjacent pane
previews.

## Merge gate

This work is suitable to replace the current terminal architecture only when:

- all configured interactive providers spawn, resume, receive input, report
  status, and terminate through Zellij on Windows;
- provider exit and server loss expose the existing recoverable lifecycle
  action, while attached-client loss recovers on the next activation or spawn;
- no stale pane can receive input or retain a memory capability;
- Agents and Agent Session use at most one agent xterm.js renderer;
- old per-agent PTY and many-renderer production paths are removed;
- documentation describes the behavior that native tests prove; and
- the complete required CI matrix passes.

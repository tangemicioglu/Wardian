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

Remote terminal attachments follow runtime generations. Termination suspends
the old feed cursor; after replacement, the existing authenticated socket
registers a fresh presentation and consumer before it resumes output. It must
not poll a removed consumer or expose that internal broker transition as a
repeating gateway error.

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
- Keep authenticated remote terminal transport on the existing per-agent
  broker. Remote clients do not attach to Zellij's private protocol and do not
  change provider-process ownership.
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

The attached client is a hidden lifecycle process that keeps the Zellij server
and provider panes alive; it is not a terminal-session broker runtime on any
platform. Each provider pane has one JSON snapshot subscription and its own
generation-scoped broker runtime. Exactly one app-level xterm.js host renders
the selected agent's broker stream. That host remains mounted in one app-root
viewport for its entire lifetime; card changes reposition the viewport without
reparenting, remounting, disposing, or recreating its xterm/WebGL objects. Agent
cards that do not own that viewport show read-only broker snapshots and consume
no xterm.js or WebGL instance. Pointer or keyboard focus on a card implicitly
focuses and fullscreens its pane before input is enabled; renderer ownership is
never a user-visible mode or action.
At most one xterm.js renderer and one WebGL context are live for agent
terminals, regardless of agent count. The singleton makes one WebGL attempt
for its lifetime. If WebGL is unavailable or its context is lost, the same
xterm instance remains on the DOM renderer instead of cycling GPU contexts.

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
7. Retain one neutral control pane so closing the last provider pane does not
   terminate the private Zellij session before a same-session restart.

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
2. Serialize focus handoffs, focus the pane by ID, and make it fullscreen.
3. Return the selected agent session ID as the broker session to present.
4. Reposition the lifetime-stable singleton viewport over the selected
   presentation without moving its DOM subtree.
5. Apply the selected pane's next complete snapshot to that host.
6. Focus the retained xterm helper as soon as the replacement frame is visible.
   Printable, control, navigation, and paste input received by the focused card
   while that bounded handoff is pending is buffered and submitted once the
   singleton owns the selected pane; it is never sent to the previous pane.
   Buffering starts only for a running, interactive card that is authorized to
   request desktop ownership. A foreign broker owner or a lifecycle transition
   to read-only, suspended, exited, or Error clears unsent input immediately.
   One handoff retains at most four input events and 4096 UTF-8 bytes for five
   seconds. A superseding selection, input-buffer expiry, or rejected broker
   delivery discards the remainder instead of retrying or replaying stale text.

The frontend queues activation requests. A later selection cannot overtake an
earlier in-flight focus command and leave the singleton renderer associated
with a different pane than Zellij has focused. Every native focus request has a
unique handoff token. The engine records each request before attached-client
startup and rechecks that token under the focus lock, so a timed-out request
cannot focus after a newer request has completed. Removing an in-flight target
invalidates that activation and queues a focus reconciliation for the still-live
target. A removed final target clears its active agent identity, so remounting
requires a fresh activation instead of silently adopting stale focus.
Preview polls are request-ordered per agent. Only the newest success or failure
may update card state, and the queued activation preflight independently
requires the selected slot to remain `running` before native focus begins.

Snapshot cards never resize panes. The singleton xterm fits the canonical
Zellij frame locally. The previous frame is hidden during a binding change and
input remains gated until the selected generation's complete frame is applied.
Authenticated remote clients may acquire the broker input lease, but they
cannot focus the desktop singleton or resize the canonical Zellij pane.
Remote resize requests are rejected with `fixed_geometry`.
During the two-phase lease transfer, Wardian hides the singleton, makes it
read-only, and disables every card for the agent until commit or rollback. Live
broker observations take precedence over preview polls at the same runtime
generation and lease epoch.

A failed running-agent restart restores its previous provider-input generation
and readiness as part of the same rollback. Wardian writes an atomic recovery
record before changing SQLite. If SQLite restoration fails, startup hydration
applies that record ahead of the stale candidate generation and retries the
write or deletion; it never advertises the candidate as recovered readiness.
Hydration first verifies that the durable agent still exists. A rollback marker
whose agent was deleted is inert and removed on the next successful recovery
file write, so cleanup failure cannot recreate the agent's readiness.

If the provider pane survives but its broker actor has terminated, restart still
uses the replacement transaction. Staging records that there is no displaced
actor; rollback returns to that actor-less state, while commit publishes the
candidate as a newly started runtime before retiring the old provider pane.

### Renderer or Workbench restart

Zellij and the backend subscriptions remain alive. The singleton renderer
registers again and replaces its local state from the selected agent's broker
snapshot. Provider and pane generations do not change.

Wardian does not authorize an unregistered pane after a backend process
restart. The app-lifetime Windows Job Object terminates the old process tree.
If the private Zellij session still answers, startup reconciliation closes
every `wardian:*` pane that is absent from the process-local generation
registry before any provider can spawn. Wardian then starts configured live
agents with new pane and memory-capability generations.

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

Lifecycle teardown awaits the generation-scoped lease close and polls the
authoritative pane list before a same-session replacement can start. Drop is a
tracked fallback: it synchronously changes the binding to `closing` and starts
an isolated cleanup worker, but never removes the binding on command success
alone. A failed or unconfirmed Zellij close therefore stays registered as
`closing`; the next spawn closes every tracked closing generation and confirms
each pane is absent before it removes that cleanup record or allocates a new
generation. If a closing generation has no pane identity, Wardian closes every
unregistered same-title pane while preserving every identified live or retired
generation. A failed replacement cancellation retains its reservation until
candidate closure succeeds, so an ordinary start cannot bypass the cleanup
record. The same rule covers failures while opening the pane subscription
transport. If clear aborts before termination, Wardian moves the original pane
lease back with the restored runtime instead of dropping it.
Restart preflight leaves the old runtime and pane untouched. A running restart
reserves a second pane generation and stages its broker actor while the old
pane, provider, broker actor, and `ActiveAgent` remain alive. The candidate is
not the active pane until its transport exists and the durable replacement
journal is ready to install the new `ActiveAgent`. Any spawn, lease, or durable
commit failure rolls the candidate broker and pane generations back and leaves
the displaced runtime usable. After the durable state commit, Wardian commits
the staged broker actor, promotes the candidate pane generation, then closes
and authoritatively confirms removal of the displaced pane. An Off agent has no
runtime to preserve and uses the ordinary start path.

Closing a Workbench surface never closes a pane. Closing the final surface
keeps the singleton xterm/WebGL allocation mounted but changes its broker
presentation to hidden and read-only, which releases input ownership. Reopening
a surface requires fresh broker presentation registration and activation while
retaining the existing Zellij pane and singleton renderer.

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
- A nonactive Terminal card shows a live, noninteractive pane preview with no
  renderer-ownership label or action.
- Pointer or keyboard focus atomically selects the card's pane and positions
  the single live terminal viewport over that card or Agent Session surface.
  Input typed during the bounded selection handoff is buffered for that pane,
  so the first printable or control key is not lost. Read-only, suspended,
  failed, or foreign-owned cards never buffer input. From the user's
  perspective, the card is simply the terminal.
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
  context, with no terminal event gaps;
- repeated focus handoffs retain the same xterm instance, WebGL addon, canvas,
  and app-root DOM host without disposal or promotion churn;
- loss of the one WebGL context falls back to the DOM renderer and restores the
  current Zellij frame without restarting or mutating any provider pane.

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

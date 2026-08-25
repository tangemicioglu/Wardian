# PTY Lifecycle and Process Integrity

Wardian is built to handle multiple simultaneous, long-running agent sessions with strict resource and process isolation.

## Cross-Platform terminal layer

Wardian bundles Zellij 0.45.0 as its agent terminal engine. Zellij owns one
provider PTY per agent pane and uses ConPTY on Windows and Unix PTYs on Linux
and macOS. On Windows, Wardian starts one attached Zellij client in a hidden
native console and tracks its PID. On Linux and macOS, Wardian uses
`portable-pty` for one attached Zellij client. Wardian does not use
`portable-pty` to start a provider process per agent.

- **Windows**: Zellij owns provider **ConPTY** instances. Wardian does not open
  them through `NativePtySystem`.
- **Linux/macOS**: Uses the standard Unix PTY system.

The standalone human terminal continues to use its own `portable-pty` runtime.

## 🛡️ Process Integrity (Windows Job Objects)
To prevent orphaned provider and console-host processes when Wardian crashes or is force-closed, the Windows implementation uses **Job Objects** via the `win32job` crate.

1. On startup, Wardian creates an app-lifetime `win32job::Job`.
2. The `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` flag is enabled.
3. Wardian assigns the backend process to that job before starting the attached Zellij client.
4. The Zellij server, launchers, provider CLIs, ConPTY console hosts, and descendants inherit the job from process creation time.
5. When the Wardian process terminates, the job object is closed by the OS, which automatically kills all processes assigned to it.

Per-agent process-tree termination is still used for normal UI actions such as kill, pause, resume, and clear. Per-agent Job Objects are only a fallback if app-level supervision cannot be installed, because post-spawn assignment is inherently less reliable than inheriting the app-level job at creation time.

At startup, Wardian also sweeps stale persisted interactive sessions before restoring agents. This catches process trees from older builds or from environments where Windows refused app-level job assignment. The sweep uses Wardian session command-line markers and `WARDIAN_SESSION_ID` environment markers, and skips agents that are off or database-marked as headless.

## 🔁 Spawning Lifecycle
Spawning an agent follows a deterministic sequence in `manager::spawn_agent`:

1. **Ensure engine**: Start or reattach the one Wardian-owned hidden Zellij
   lifecycle client. It does not register a desktop broker session.
2. **Build provider command**: Assemble the executable, exact argument vector,
   working directory, and per-agent environment.
3. **Create one-use manifest**: Persist a nonce-bound JSON manifest containing
   the exact provider launch specification.
4. **Create pane**: Ask Zellij to create the pane with Wardian's bundled
   terminal host. The host validates and deletes the manifest before it starts
   the provider.
5. **Subscribe**: Observe replacement pane frames and provider-native logs.
6. **Register runtime**: Register the pane-addressed transport under the agent's
   existing terminal session ID in the terminal-session broker.
7. **Register binding**: Store the generation-scoped agent-to-pane binding in
   `AppState`.

### Shell-hosted Launch Notes
- Workflow shell-command nodes and headless provider runs use the same shell resolver as interactive PTY sessions.
- On Windows, provider shims are host-aware: PowerShell hosts invoke `.cmd` and `.bat` shims directly through PowerShell, while POSIX-like hosts such as Git Bash or WSL may route Windows shims through `cmd.exe` for compatibility.
- On Linux and macOS, Wardian resolves shells from the standard shell list and executes the provider command through that shell's command-string mode.

## Input Readiness and Interaction Delivery

Terminal input is a transport, not Wardian's communication source of truth.
The interaction control plane owns structured messages, asks, replies,
delivery attempts, and Inbox evidence. Structured delivery addresses the
agent's Zellij pane under the existing per-agent lock.

Each interactive provider runtime has a provider input generation. The generation increments whenever Wardian creates or reattaches a runtime boundary, including spawn, resume, clear, and provider reattach. Readiness observations are valid only for the generation that produced them.

```text
ProviderInputState {
  session_id,
  generation,
  state: unknown | booting | ready | busy | action_required | unavailable,
  ready_evidence: provider_event | prompt_detected | title_detected | manual_status,
  observed_at
}
```

Delivery follows these rules:

- Ready evidence for the current generation can drain queued interaction delivery.
- Booting, busy, action-required, unavailable, or missing input-sender states keep delivery queued with a precise reason.
- Readiness or status from an older generation cannot drain queued work for a newer runtime.
- Provider action-required status remains provider-owned. It usually represents a provider permission or authentication prompt, not a Wardian human-in-the-loop interaction.
- Codex readiness can use prompt detection as release evidence, but it must not depend on a fixed sleep before text injection.

This model prevents first-input races where Wardian writes into a provider before the provider prompt is actually ready. It also keeps Inbox and CLI behavior tied to durable interaction and provider events rather than terminal repaint artifacts.

## Testing Boundaries

PTY behavior cannot be validated by browser-only UI tests.

- Browser Playwright smoke tests are useful for layout, navigation, and non-native UI regressions.
- Native Tauri runtime tests are required for:
  - Tauri `invoke` behavior
  - PTY-backed terminal rendering
  - provider spawn and resume behavior
  - shell-hosted process launch behavior

When debugging or testing PTY issues, treat browser smoke results as insufficient evidence. Use the native runtime harness for any claim about terminal or provider behavior.

## 📐 Terminal Resizing

Zellij derives provider-pane geometry from its attached client and layout.
Snapshot cards never resize panes. The active desktop xterm fits the canonical
Zellij frame locally. Presentation resize requests do not resize a provider
ConPTY in this replacement, so narrow and duplicate surfaces cannot destabilize
the provider TUI. The terminal broker keeps its parser at the same canonical
geometry; viewport reports remain local presentation data and cannot reinterpret
a complete Zellij frame at a different width.

See [Terminal Presentation Broker](./terminal-presentation-broker.md) for the
generation, lease, snapshot, and ownership-transfer protocol.

## 🖥️ Frontend Terminal Runtime

Wardian's frontend terminal stack is built on `xterm.js` and is intentionally treated as a runtime layer, not just a view component.

### Renderer Strategy

- Wardian uses xterm's WebGL renderer for mounted terminal views when available. WebGL is preferred because xterm's `customGlyphs` support for block and box-drawing characters does not apply to the DOM renderer, and provider TUIs such as Claude Code rely on those glyphs for mascot/status rendering.
- If WebGL is unavailable or loses its context, Wardian falls back to xterm's built-in DOM renderer rather than failing terminal initialization.
- Renderer instances are not the source of runtime truth. One lifetime-stable
  app-root renderer presents the selected pane's broker stream; inactive agent
  cards use replacement pane snapshots and allocate no xterm.js or WebGL
  instance. Card focus repositions the stable viewport and changes its
  generation-scoped broker binding without reparenting its DOM, recreating its
  xterm, or cycling its WebGL addon.
- Renderer retirement is lease-bound. Output/reset/refresh operations capture
  one renderer identity before awaiting; retirement releases its budget slot
  immediately but defers physical disposal until every in-flight operation
  finishes. Post-await work may mutate only the captured renderer generation.
- Provider integrations must not depend on renderer-specific behavior.

### Capability Handling

Terminal capability negotiation is centralized in `src/features/terminal/terminalCapabilities.ts`.

That layer is responsible for responding to standard terminal queries such as:

- device status reports
- resize and pixel-size queries
- DECRQM mode checks
- OSC palette queries
- OSC 10/11 foreground and background color queries
- synchronized output toggles

Provider-specific terminal adapters should only exist when a provider genuinely requires non-standard behavior. Capability replies should otherwise be implemented once in the shared terminal layer.

### Broker Snapshot and Replay Model

Wardian preserves terminal state across presentation remounts and independent
desktop/remote renderers while the PTY runtime is live.

That means:

- switching tabs
- zooming or restoring groups
- remounting the terminal component

should not discard the active terminal buffer.

Terminal contents are runtime state and are never written into the workbench
document. A process restart can restore the tab but not a terminated PTY's
screen contents.

The session model is split into two layers:

- a Rust broker parser that continuously receives PTY output and owns canonical
  in-process screen, geometry, bounded snapshot, and replay state;
- independent mounted presentation terminals that consume one ordered stream
  and can be disposed and reconstructed from a snapshot/barrier.

When a presentation remains resident within the process-wide budget, Wardian
reuses its renderer across tab, layout, and viewport transitions. If it was
suspended or evicted, the presentation
applies a fresh bounded broker snapshot, discards events at or below the
snapshot barrier, and then replays consecutive later events. It must resync
again on a cursor gap or generation change.

### Redraw and Scrollback Normalization

Some TUIs repaint by moving the cursor home and rewriting the current viewport instead of using the alternate screen buffer. Wardian normalizes the cases that would otherwise diverge from user expectations:

- A clear-screen preamble made from many `EL + newline` writes followed by cursor-home is treated as a real clear-and-home operation. This prevents TUI redraws, such as Claude's mascot frame, from being copied into scrollback during maximize/restore.
- Synchronized home-redraw TUIs are marked as transient screen renderers. Before a row-shrinking resize, Wardian moves the local xterm cursor home so xterm does not promote the old visible TUI frame into scrollback before the provider redraws at the new size.
- After any resize, Wardian arms one duplicate-redraw suppression window. If the next synchronized home-redraw batch is mostly already present in the parser buffer, Wardian drops that repaint instead of letting xterm append a second copy of the same transcript to scrollback.
- Codex interactive sessions use its documented `--no-alt-screen` inline mode, and Wardian journals overlapping home-redraw frames into xterm scrollback. Codex still emits a sliding viewport, so Wardian reconstructs dropped frame lines before applying the next repaint.

### PTY Output Batching

The frontend drain path batches PTY output before writing into xterm instead of issuing one write per small chunk. This reduces render pressure during bursty output and improves scrolling behavior for TUI-heavy providers such as OpenCode.

# Agent Browser Surface

Filename: `2026-08-09-agent-browser-surface.md`

- **Status:** Implemented (phases 1, 2, 3, and 4a; 4b outstanding, 4c deferred). Phases 1 and 2 landed in #869 after a review loop that converged at round nine; phase 3 landed in #874, and phase 4a in #877.
- **Date:** 2026-08-09

## Delivery status

| Area | Outcome |
|---|---|
| Engine | `state/browser_session/engine.rs` discovers Edge → Chrome → Brave → Chromium, or `WARDIAN_BROWSER_BINARY`, and launches it headless with an isolated profile on an ephemeral loopback debug port. |
| Protocol | `cdp.rs` is a ~250-line CDP client: request/response correlation, flattened target sessions, an event broadcast, and a 30s per-call ceiling. `tokio-tungstenite` was already in `Cargo.lock` via axum, so no new dependency tree. |
| Session runtime | `actor.rs` owns the broker and per-session state: navigation, history, `get`, `wait`, snapshot, actions, screenshots, viewport, `eval`, console capture, screencast, and input forwarding. |
| Refs | `snapshot.rs` mints `e1..eN`, stamps them on the DOM, and gates every action on three checks — generation, uniqueness, and element identity — each of which refuses rather than guesses. |
| Control plane | 12 `ControlRequest` variants; browser error codes are carried to the CLI intact rather than flattened to `generic`. |
| CLI | `wardian browser open\|list\|<target> …` with `--json` and `--snapshot-after`. Ownership defaults to `WARDIAN_SESSION_ID`. |
| Surface | `features/browser/BrowserSurface.tsx` renders the screencast, forwards pointer/wheel/keyboard, and stops streaming when hidden. Registered as a `Sessions` contribution that provisions its own session. |
| Lifecycle | Sessions close on explicit close, on the owning agent's `kill_agent`, and on app exit. Closing a tab only detaches. A failed launch leaves no profile behind; a crashed browser publishes a closed event. |
| Drive lease | Attaching mints an opaque token; the first attachment drives and later ones mirror read-only. Every surface-originated mutation, navigation and viewport included, carries the token, and an omitted one is refused rather than waved through. |
| Introspection | `network.rs` folds `Network.*` events into a bounded ledger that navigation does not clear; cookies, `localStorage`/`sessionStorage`, and downloads are first-class commands. Downloads live beside the profile rather than inside it, so closing a session never takes the file. |
| Restore | A surface whose session is gone reopens its persisted page the first time it becomes visible, at the persisted viewport. A crash, a hidden tab, and a blank address stay manual. |

Verification: 77 Rust unit tests, 37 `#[ignore]`d engine-backed integration
tests against real Edge, 33 CLI unit tests, 29 core wire-type tests, 51 frontend
tests, and two native E2Es — one that provisions a session from the launcher,
renders a screencast frame, and drives the page through the CLI, and one that
restarts the app and finds the tab loaded again under a new session. Phase 4b
below is not built; 4c is deferred.

## Review round

An autoreview pass (blueprint `autoreview`, run `1786270694884-d61f0e31`)
returned six findings against the first commit. Five were real; each is fixed
with an engine-backed regression test.

| Finding | Outcome |
|---|---|
| A recycled DOM node could be acted on through a ref that had not gone stale | Actions now pin the snapshot generation in the selector, require exactly one match, and re-derive the element's role and name to compare against what the snapshot recorded. New codes `ref_changed` and `ref_ambiguous`. |
| A dropped `Page.frameNavigated` on the lossy event channel would leave stale refs valid | `RecvError::Lagged` now invalidates every ref and resynchronizes, because the pump cannot know what was discarded. |
| A failed first navigation returned an error while leaving a live browser registered | Treated as a page outcome: the session stays usable and reports `load_state: failed`. A failed launch now also removes its profile directory. |
| Mirrored presentations could drive the shared page, and the chrome bar bypassed `read_only` entirely | A drive lease assigned on screencast attach, enforced in the backend and reflected as disabled controls in the surface. |
| A screencast attach resolving after teardown left the stream running | The effect tracks cancellation and detaches a late attach. |
| Same-document navigation (`pushState`, hash) left the URL and refs on the previous route | `Page.navigatedWithinDocument` now updates the URL and invalidates refs. |

The sixth item, a residual note that the review did not independently verify
Chromium's loopback bind for `--remote-debugging-port=0`, is unresolved by
inspection and left as a stated assumption.

A second round (run `1786272581592-5f36bd03`) found four more, all real, in the
fixes themselves. Each is fixed with an engine-backed regression test.

| Finding | Outcome |
|---|---|
| The lease was frontend-only: `navigate` and `viewport` had no check at all, and an omitted presentation id was authorized as the control-plane path | Attaching mints an opaque token. Every surface mutation carries it; omitting it is a refusal. Navigation and viewport are gated too. |
| A failed `Page.startScreencast` left a ghost owner, so later attachments skipped the start and mirrored an invisible driver | The attachment is rolled back when the stream fails to start. |
| A late attach cleanup could detach a newer attach for the same presentation | Attachments are keyed on their own token, so a stale cleanup releases only itself. |
| A failed open could leave a locked profile directory, because `kill_on_drop` terminates without reaping and Windows holds the lock during termination | `open` owns the child across the fallible region and reaps it before removing the profile. |

A third round (run `1786275258071-92d03395`) found four more, again all in the
previous round's work.

| Finding | Outcome |
|---|---|
| Screencast start/stop was not serialized, so a concurrent attach could skip a start that then rolled back, and a detach's stop could land after a concurrent start | Attach and detach hold a per-session transition lock across their CDP call. |
| A crashed browser stayed registered, so `browser list` kept showing it and later commands resolved a dead connection | The pump reaps the session out of the broker before announcing, and only the remover announces. |
| Provisioning could orphan a live Chromium when the store rejected the open it was created for | Provisioning returns a `release`, called when the open fails. |
| A CLI open that beat the frontend listener lost its only surface notification | Non-detached opens are queued as well as emitted, and drained after the listener is installed. |

Fixing the reap exposed a further defect: teardown called `Page.close` on a
dead connection and blocked for the full 30-second call ceiling. The connection
now latches closed and fails later calls immediately, which also halved the
engine-backed suite's runtime.

Reviewing the fixes also surfaced one defect the round did not name: the ledger
records clamped fields while the new identity guard compared the raw name, so
any element with a name over the 160-character cap would have refused itself as
`ref_changed`. The guard now clamps identically, proven by an engine-backed
test against a deliberately over-long label.

A fourth round (run `1786276499101-b4fa266d`) found five, all in the third
round's lifecycle work. Every one is a race the engine-backed suite could not
force on its own.

| Finding | Outcome |
|---|---|
| A browser that died between `spawn_event_pump` and the broker insert was reaped before it was registered, so the reap found nothing and `open` then inserted a dead session with no pump left to remove it | The session is registered before its pump starts, and the pump re-checks `is_closed()` once before its first `recv` so the subscribe-to-recv window cannot strand it either. |
| `CdpConnection::dispatch` checked the closed latch before inserting its pending sender, so a disconnect landing in between left a call waiting out the full 30-second ceiling — and with the transition lock held, blocking every later attach/detach | The closed check moved inside the `pending` lock the reader drains under, so a disconnect either drains the sender or is visible to the check. |
| `close` and `close_for_agent` announced unconditionally, so a crash winning the race to remove the entry produced two contradictory closed events and a double teardown | Every teardown path goes through one atomic `take_session`; only whoever gets the session back announces and shuts down. |
| `reopenBrowserSurface` orphaned its replacement Chromium when `rebind_resource` returned a non-`allow` decision or threw — the same leak class round three closed for launcher provisioning, on the open-then-commit path | The reopen closes the session it just created on both a rejected decision and an exception. |
| The pending-surface-open queue had no listener-ready state, so every normal CLI open stayed queued for the app's lifetime and a remount replayed stale work | Opens are queued only before the first drain; the drain marks the listener ready, and a `MAX_PENDING_SURFACE_OPENS` ceiling bounds a pre-readiness burst. |

A fifth round (run `1786296198062-0a6f55f8`) found two, both real, both in the
handoff between a live runtime and the frontend that presents it. No finding
recurred from an earlier round.

| Finding | Outcome |
|---|---|
| The pending-open queue was disabled permanently by the first drain, so any later gap with no listener — a webview reload, an effect re-subscription — silently dropped a CLI open | Registration returns an epoch and the cleanup releases it, so queueing tracks whether a listener exists right now. A release for a superseded epoch is ignored, since React can mount the replacement before the outgoing cleanup lands. |
| A session closing between the surface's initial lookup and its listener being installed missed the only `closed` event, leaving a resolved pane with no frame, no lease, and no Reopen | The surface re-checks the session once the listener is installed. |

A sixth round (run `1786297668296-80776254`) found one, in the retirement half
of the fifth round's fix.

| Finding | Outcome |
|---|---|
| Cleanup removed the event listener before the release reached the broker, so an open landing in that window was emitted to nobody and queued by nobody — and a disposal during an in-flight registration had the same gap | Retirement releases first and unlistens only after the release is acknowledged, so the listener always outlives the registration. The lifecycle moved into `subscribeToBrowserSurfaceOpens` to make the ordering testable. |

Reviewing the fix surfaced one the round did not name: the cancelled branch
drained the backend queue and then discarded what it got, losing opens nothing
would replay. A drain that resolves after disposal is now surfaced anyway,
which is safe because opening a surface is idempotent by resource key.

A seventh round (run `1786298645199-5e7ea9e2`) found two.

| Finding | Outcome |
|---|---|
| A rejected release still unlistened, so a failed teardown invoke reopened the same no-listener gap round six closed | The listener handshake is gone. Every open stays recorded until a frontend acknowledges it, so no delivery decision depends on a message arriving. |
| `Page.navigatedWithinDocument` was not filtered by frame, so an iframe changing its hash rewrote the session URL and invalidated the top-level page's refs — contradicting the documented iframe rule | The event is compared against the main frame id, read from `Page.getFrameTree` at attach and refreshed on every main-frame commit. |

The first finding was the third variant of one shape: three rounds in a row
found a window where the backend believed a frontend listener existed while
none did. Narrowing the window again would have invited a fourth, so the
handshake was replaced rather than repaired. `queue_surface_open` now always
records; `pending_browser_surface_opens` reads without consuming; and
`ack_browser_surface_open` is the only thing that removes an entry. Repeat
delivery is the accepted cost, and `focus_resource` already made it harmless.

An eighth round (run `1786300135026-80c61a66`) found one, in the new
acknowledgement protocol's interaction with app startup.

| Finding | Outcome |
|---|---|
| The frontend subscribed before the durable workbench document had loaded, so a CLI open arriving during boot was applied to the provisional document, acknowledged, and then erased by `adopt_durable_state` — live browser, no surface, nothing left to replay | The subscription waits for `workbenchPersistence.status === "ready"`, the same gate `useArtifactEvents` already used. Waiting is free because the backend holds every open until it is acknowledged. |

A ninth round (run `1786301270826-79349592`) returned no findings and a verdict
of approve, which ends the loop. It confirmed that the readiness gate runs
after `adopt_durable_state`, that the subscription listens before it reads, and
that acknowledgement follows the workbench mutation rather than preceding it.

Two review-process notes worth keeping. Three consecutive rounds found the same
shape of defect — a window in which the backend believed a frontend listener
existed while none did — each one narrower than the last. That pattern is the
signal to replace a design rather than repair it, which is what round seven
did. And twice a first-draft regression test passed against the very defect it
was written for: once from an assertion that ran before the event pump had
processed anything, once from an arity mismatch that made the assertion vacuous.
Both were caught by reverting the fix and requiring the test to fail. That step
is not optional here.

### What differs from the plan below

- **WebView2's `msedgewebview2.exe` is not in the discovery order.** It is not supported as a standalone browser, and every Windows 11 host that can run Wardian already has Edge proper.
- **`Target.createTarget` is called without width/height.** The protocol only accepts a size alongside `newWindow`; the viewport is established by `Emulation.setDeviceMetricsOverride`, which is what the screencast follows.
- **Contributions gained `provisions_resource`.** `requires_resource` greys an entry out until the caller supplies a key, which would have made Browser unopenable from the launcher. Provisioning entries instead create their resource first, through a `provision_surface_resource` hook on `WorkbenchHost`.

## Context and Problem Statement

A Wardian agent can edit code, run it, and read the terminal. It cannot look at
the result. Any change whose evidence lives in a rendered page — a frontend fix,
a dev-server route, a docs page the agent needs to read — ends its verification
loop at "the build passed", which is the weakest claim the agent could make.

The workbench already anticipated this. `coreSurfaceRegistry.ts:63` registers a
`browser` contribution marked `reserved: true` with the description "Reserved
for a future browser contribution." `OpenSurfaceDialog.tsx:40` greys it out.
This spec fills that reservation.

There are two reference implementations, and the distinction matters.

[cmux](https://github.com/manaflow-ai/cmux) ships a browser pane as a peer of
its terminal panes and lets agents drive it through the same CLI a human uses.
Its README states that "the in-app browser has a scriptable API ported from
[agent-browser](https://github.com/vercel-labs/agent-browser)", so cmux is the
downstream port and
[`vercel-labs/agent-browser`](https://github.com/vercel-labs/agent-browser) is
the design ancestor.

cmux contributes the *surface model*: a browser as a first-class pane in a
terminal-centric workbench, addressed and driven like any other. agent-browser
contributes the *engine and command model*: a Rust CLI over a Rust daemon
speaking CDP directly, with snapshot refs (`@e1`, `@e2`) instead of selectors.
Wardian takes the surface model from cmux and lands much closer to
agent-browser on the engine — which was arrived at from the constraints below
before the lineage was traced, and is documented here so the attribution is
right rather than to claim independent invention.

## What cmux actually does

Read from cmux's README and
[`skills/cmux-browser/SKILL.md`](https://github.com/manaflow-ai/cmux/blob/main/skills/cmux-browser/SKILL.md).
These are cmux's published docs, not its source; the design decisions below are
what the documented interface implies, and each is worth adopting on its own
merits rather than because cmux does it. Note that the agent-facing control
ships as a *skill*, the same packaging Wardian uses for
`resources/library/skills/wardian-skills/wardian-cli/`.

| cmux behavior | Evidence | Wardian equivalent today |
|---|---|---|
| Browser is a pane type in the normal split/tab layout, restored on relaunch | README: "a real browser pane"; sessions restore "layout, directories, scrollback, and browser history" | Workbench surfaces + Dockview + `workbenchPersistence.ts` |
| Surfaces addressed as `surface:N`, UUIDs accepted on input, `--id-format uuids\|both` | SKILL.md | No short-ref convention; CLI resolves agents by UUID or name |
| `browser open` scopes to the calling terminal's workspace via `CMUX_WORKSPACE_ID` | SKILL.md | `WARDIAN_SESSION_ID` is already injected into every agent PTY (`commands/agent.rs:3740`) |
| One CLI serves human and agent: `open`, `get url\|text\|html`, `wait`, `snapshot --interactive`, `fill`, `click`, `press`, `scroll`, `viewport` | SKILL.md | `wardian-cli` with a `ControlRequest` enum over a Unix socket / named pipe |
| Actions target snapshot refs (`e1`, `e2`), not selectors; refs go stale after navigation or DOM change | SKILL.md | — |
| `--json` machine output and `--snapshot-after` to fold a re-snapshot into an action | SKILL.md | `output.rs` already has a JSON mode |
| WKWebView engine, so CDP-only capabilities return `not_supported`: offline emulation, trace/screencast recording, network interception, raw input injection | SKILL.md "Limitations" | — |

Six decisions transfer:

1. **One CLI, two consumers.** No separate agent-only API to drift.
2. **Addressable, stable surface identity.** Short ref for humans and prompts,
   UUID as the canonical form.
3. **Ambient scoping from the caller's environment**, with explicit override
   flags. An agent should not have to know its own ID to open a page.
4. **Ref indirection instead of raw selectors.** Bounded output, and actions
   that name something the agent has actually seen.
5. **Declare unsupported capabilities explicitly.** cmux returns
   `not_supported` rather than emulating badly. That honesty is the feature.
6. **Panes are peers.** The browser is not a modal or an aside.

The one decision *not* to transfer from cmux is the engine — and that is a
divergence from the port, not from the lineage. cmux is a native Swift and
AppKit macOS app, so WKWebView is the right call there; the `not_supported`
returns above are the price of that platform choice, since the API it ported
was written against CDP. agent-browser itself drives Chromium over CDP from a
Rust daemon, which is where Wardian ends up. Wardian is Tauri on Windows-first,
so the constraints push the same way.

## Engine decision

| Option | Verdict |
|---|---|
| **A. Tauri child webview** (`tauri = { features = ["unstable"] }`, `window.add_child`) — the closest analogue to cmux's WKWebView | **Reject** |
| **B. Out-of-process Chromium over CDP, composited into the DOM as a screencast** | **Adopt** |
| **C. `<iframe>`** | **Reject as the architecture** |

**Why not A.** Tauri's multi-webview support is explicitly WIP behind an
`unstable` flag, and its open defects land on Wardian's primary platform:
`add_child` deadlocks when called from a synchronous command or event handler
on Windows ([#10236](https://github.com/orgs/tauri-apps/discussions/10236)),
child webviews render white on load
([#10011](https://github.com/tauri-apps/tauri/issues/10011)), and positioning
desynchronizes after maximize/restore
([#11170](https://github.com/tauri-apps/tauri/issues/11170)). Beyond the bugs,
a native child webview composites *above* the host DOM. Dockview's tab strip,
the command palette, `WorkbenchMruSwitcher`, every dialog, and the drag preview
would all be occluded by the browser pane, and each would need a native-aware
escape hatch. Automation would be limited to script evaluation, so cookies,
network, console, and downloads would each be hand-built.

**Why not C.** The app's CSP is `null` (`tauri.conf.json:27`), so Wardian
permits the frame — but `X-Frame-Options` and `frame-ancestors` from the remote
origin still refuse it, which rules out most of the web, and there is no
automation surface at all. It is adequate only for a localhost dev-server
preview, and option B renders localhost equally well.

**Why B.** The browser becomes a backend-owned runtime resource with a
lifecycle shaped exactly like a PTY session, which is the arrangement the
architecture already mandates ("the Rust backend is the definitive authority
for agent session lifecycles"). Concretely:

- The pane content is a DOM element, so z-order, Dockview, group zoom,
  `suspend_when_hidden`, and remote/PWA mirroring all work unchanged.
- CDP gives the full capability set — snapshot, cookies, storage, console,
  network, downloads, screenshots. Every capability cmux has to return
  `not_supported` for, Wardian gets from the protocol, because those were
  CDP-only capabilities the WKWebView port had to drop.
- `tokio-tungstenite 0.29` is already resolved in `Cargo.lock` via axum's `ws`
  feature, so a CDP client adds no new transitive dependency tree.
- No X-Frame-Options problem, because it is a real browser.

Costs, stated plainly: it needs a Chromium on the machine; screencast frames
add latency a local webview would not; pointer and keyboard mapping to
`Input.dispatch*` is real work; and a second browser process is visible in the
task manager.

## Architecture

### Backend: browser session as a runtime resource

New module `src-tauri/src/state/browser_session/`, mirroring the structure of
`state/terminal_session/`. One actor per session.

```rust
pub struct BrowserSession {
    browser_id: Uuid,            // canonical identity, the surface resource_key
    short_ref: u32,              // "browser:3" — stable for the app's lifetime
    owner_agent_id: Option<String>,
    workspace: Option<PathBuf>,
    engine: EngineKind,
    url: String,
    title: String,
    load_state: LoadState,
    viewport: Option<Viewport>,
    snapshot_generation: u64,    // invalidated by navigation and DOM mutation
}
```

Engine discovery order, first hit wins: `WARDIAN_BROWSER_BINARY` → the WebView2
runtime's bundled Edge (present on any machine that can run Wardian at all) →
Edge stable → Chrome → Chromium. The process is launched with
`--remote-debugging-port=0` bound to loopback and an isolated `--user-data-dir`
under the session's own directory, so an agent-driven browser never inherits
the human's cookies or logged-in sessions.

The session reuses the terminal broker's presentation model verbatim:
`owner_presentation_id` plus mirrors. `AgentSessionSurface.tsx:53-59` already
documents the governing contract for terminals — "unmounting or closing a tab
can detach this renderer without pausing, clearing, or terminating the shared
runtime" — and the browser session must honor the same rule. A session ends on
explicit close, on owning-agent termination, or on app exit. Never on tab close.

### Surface definition

```ts
surfaceDefinition({
  type: "browser",
  title: "Browser",
  render_policy: "suspend_when_hidden",   // stop the screencast, keep the page alive
  open_policy: "focus_resource",
  runtime_policy: "runtime_backed",
  resource_key: (request) => requireBrowserId(request),
  presentation_title: (surface) => page title, falling back to the host
  badges: load state, "Agent driving", console-error count
})
```

State is `{ url, viewport: { width, height } | null }` — enough to reopen the
same page after a cold restart, and small enough to stay well inside
`max_state_bytes`.

Two registry constraints shape this and are easy to get wrong:

- **`resource_key` callbacks must be pure.** `surfaceRegistry.ts:715-745`
  re-invokes `resource_key` on every restore and rejects the surface if the
  result does not reproduce the stored key. The callback therefore cannot mint
  a session. Minting happens in the navigation layer, before
  `navigation.open({ surface_type: "browser", resource_key: newBrowserId })`.
- **`requires_resource: true` disables the launcher entry.**
  `OpenSurfaceDialog.tsx:40` greys out any contribution requiring a resource
  when no ambient `resource_key` is present. Browser must therefore be
  `group: "Sessions"`, `requires_resource: false`, `reserved` removed, with the
  launcher routed through a new `workbench.open.browser` command that mints the
  session id first. Updating the contribution touches
  `coreSurfaceRegistry.test.ts` and `OpenSurfaceDialog.test.tsx`.

On a cold restore the stored `browser_id` will not resolve to a live session.
The surface renders the same shape as `AgentSessionSurface`'s missing-agent
placeholder, with a "Reopen this page" action that mints a session at the
persisted URL and rebinds the surface.

### Frontend

- `src/features/browser/BrowserSurface.tsx` — chrome bar (back, forward,
  reload/stop, URL field, agent-control indicator), the screencast viewport,
  and a status footer carrying load state and console error count.
- `src/features/browser/browserSessionClient.ts`, modeled on
  `terminalSessionClient.ts`: attach/detach, owner/mirror negotiation, event
  channel.
- Input forwarding translates pointer, wheel, and keyboard events to
  `Input.dispatchMouseEvent` / `Input.dispatchKeyEvent`. Suppressed when the
  presentation is a mirror or while an agent holds the drive lease, reusing the
  `interaction_capability: "read_only"` path the terminal surface already has.
- Registered in `App.tsx`'s `renderWorkbenchSurface`, beside `agent-session`
  and `files`.

### Control plane and CLI

New `ControlRequest` variants in `crates/wardian-core/src/control.rs`, handled
in `src-tauri/src/control.rs`, exposed as `wardian browser <subcommand>` via a
new `crates/wardian-cli/src/browser.rs` shaped like `graph.rs`.

| Command | Request |
|---|---|
| `browser open <url> [--agent \| --detached] [--background]` | `BrowserOpen` |
| `browser list` | `BrowserList` |
| `browser <target> close` | `BrowserClose` |
| `browser <target> navigate <url> \| back \| forward \| reload` | `BrowserNavigate` |
| `browser <target> get url\|title\|text\|html [selector]` | `BrowserGet` |
| `browser <target> wait --load-state\|--selector\|--text\|--url-contains\|--function --timeout-ms` | `BrowserWait` |
| `browser <target> snapshot [--interactive]` | `BrowserSnapshot` |
| `browser <target> click\|fill\|press\|scroll\|hover\|select <ref> [value] [--snapshot-after]` | `BrowserAct` |
| `browser <target> screenshot <path> [--full-page]` | `BrowserScreenshot` |
| `browser <target> viewport <w> <h> \| reset` | `BrowserViewport` |
| `browser <target> console\|network\|cookies\|storage\|downloads` | `BrowserConsole`, `BrowserNetwork`, `BrowserCookies`, `BrowserStorage`, `BrowserDownloads` |

`<target>` accepts `browser:N` or a UUID, matching cmux's short-ref convention.
`wardian browser open` with no `--agent` attributes the session to
`WARDIAN_SESSION_ID`, which every agent PTY already carries, and opens the
surface in the workbench.

**Snapshots.** `snapshot --interactive` walks the accessibility tree and emits
numbered refs with role, accessible name, and value, capped in both element
count and serialized bytes. The ref table is held by the session actor against
`snapshot_generation`. Navigation or a DOM mutation bumps the generation, and
acting on a stale ref returns a `snapshot_stale` error. cmux warns agents that
refs go stale; Wardian should make staleness a refusal rather than a silent
misclick on whatever now occupies that position. This is the single most
important correctness property in the feature.

### Multi-surface handling

- **Many surfaces, one session.** Splitting a browser pane mirrors the same
  session read-only, exactly as terminals do today.
- **Many sessions, one agent.** `browser:1..N`, enumerated by `browser list`.
- **Ownership is not presentation.** Closing a tab detaches; it never kills.
- **One driver at a time.** An agent action takes a short drive lease; the
  surface shows an "Agent driving" badge and suppresses human input for its
  duration, so a human and an agent cannot race the same page.

### Phase 3: introspection

Phases 1 and 2 let an agent drive a page. They do not let it see what the page
*did*. A form submit that silently 500s produces the same DOM, the same
snapshot, and the same screencast as one that succeeded; the only evidence is a
request nobody can read. Phase 3 is about that evidence.

`console` and `eval` shipped with phase 1 because they cost almost nothing on
top of the existing protocol client. What follows is the rest.

**Network is a ledger, not a stream.** `Network.enable` goes on at attach,
beside `Page`, `Runtime`, and `Log`, and every request lands in a bounded
per-session ring. Enabling it lazily on the first `network` call was the
tempting alternative and it is the wrong one: an agent asks about the network
*after* something went wrong, and a ledger that starts recording at the moment
of the question is empty exactly when it matters.

The cost of always-on is event volume on a channel that also carries screencast
frames, where a lag does not merely drop frames — the pump cannot know whether
a `Page.frameNavigated` was among the discarded messages, so it invalidates
every outstanding ref. A page load emitting several hundred network events
makes that far more likely than phase 1 ever did. Two things keep it honest:
the CDP event channel grows to 2048, and the network arms of the pump perform
no protocol calls, so they cannot stall the loop the way the title read after
`Page.loadEventFired` can.

The ring is *not* cleared on navigation, which is the opposite of what console
does. Console errors belong to the page that produced them. Network records
belong to the agent's investigation, and the document request that starts a
navigation is emitted *before* the `Page.frameNavigated` that would clear it —
so clearing on navigation would reliably delete the single most interesting
record in the ledger. `network --clear` is explicit instead.

**Response bodies are fetched, never stored.** Headers are recorded with the
request; bodies are read live through `Network.getResponseBody` only when
`--body` asks, and only up to a cap. A body outlives its request only while
Chromium's own buffer holds it, so `--body` can legitimately fail on an old
record; it says so rather than reporting an empty body.

**No redaction.** `network <id>` prints `Authorization` and `Cookie` headers,
and `cookies` prints cookie values. Redacting them would defeat the commands —
"was the token sent?" is the question being asked — and redacting in one place
while printing in the other would be worse than doing neither. What makes this
defensible is the isolated profile: these are credentials the session itself
acquired, never the human's. The guides say plainly that this output carries
secrets and does not belong in a shared artifact.

**Storage goes through the page, not `DOMStorage`.** `Runtime.evaluate` gets
the page's own origin for free, treats `localStorage` and `sessionStorage`
identically, and needs no `storageId` plumbing. The one thing it must handle is
an origin that has no web storage at all — `about:blank`, a sandboxed frame —
where the DOM throws `SecurityError`. That becomes a named refusal pointing at
the fix, not a protocol error.

**Downloads outlive their session.** A download's whole purpose is the file
afterwards, so downloads are written to
`<WARDIAN_HOME>/browser/downloads/<browser_id>/`, a sibling of the profile
directory rather than a child of it — the profile is deleted on close, and
taking the agent's export with it would be indefensible. Growth is bounded at
the other end: the broker prunes download directories older than seven days
when it starts.

`Browser.setDownloadBehavior` runs in `allowAndName` mode, which writes each
file under its download GUID. That is deterministic but useless to an agent, so
a completed download is renamed to its suggested filename, deduplicated with a
numeric suffix. A rename that fails is reported at the GUID path rather than
failing the download.

These events are browser-scoped and carry no target session, so the pump's
session filter has to admit them. It admits *all* session-less events, because
one connection serves exactly one browser: an event with no session id on this
connection is unambiguously this session's. Events carrying some *other*
target's session id — a popup, an extension worker — stay filtered out.

**Command surface.** The grammar stays `wardian browser <target> <verb>`, and
the verbs follow [agent-browser](https://github.com/vercel-labs/agent-browser)
where a shape already exists there.

| Command | Notes |
|---|---|
| `network [--filter <text>] [--method M] [--status 2xx\|404] [--type xhr,fetch] [--limit N] [--failed]` | Most recent last. |
| `network <request-id> [--body]` | Headers both ways; body on demand. |
| `network --clear` | Empties the ledger. |
| `cookies [--all]` | Page-scoped by default; `--all` covers the browser context. |
| `cookies set <name> <value> [--url\|--domain\|--path\|--secure\|--http-only\|--same-site\|--expires]` | URL defaults to the current page. |
| `cookies delete <name> [--url\|--domain\|--path]` | |
| `cookies clear` | |
| `storage local\|session [key]` | No key lists the whole store. |
| `storage local\|session set <key> <value>` | |
| `storage local\|session remove <key>` | |
| `storage local\|session clear` | |
| `downloads [--clear]` | Lists state, size, and resolved path. |
| `console [--level error\|warning\|info] [--clear]` | |

**Bounds.** Every ledger is capped, because an agent's context is the scarce
resource and a chatty page is not an unusual one.

| Bound | Value | Why |
|---|---|---|
| `NETWORK_BUFFER` | 500 records | A page load is a few hundred requests; one full load always fits. |
| `MAX_NETWORK_URL_CHARS` | 1024 | `data:` URIs are otherwise unbounded. |
| `MAX_NETWORK_HEADERS` | 32 per direction | |
| `MAX_NETWORK_HEADER_CHARS` | 512 | |
| `MAX_RESPONSE_BODY_BYTES` | 64 KiB | |
| `MAX_STORAGE_BYTES` | 64 KiB total | |
| `MAX_STORAGE_VALUE_CHARS` | 2048 | |
| `DOWNLOAD_RETENTION_DAYS` | 7 | |

Truncation is always reported, never silent.

**Out of scope, deliberately.** Request interception (`network route`,
`--abort`, response mocking) and HAR recording. agent-browser ships both and
they are genuinely useful, but they change what the page does rather than
observe it. That is a different risk class — an agent that can silently mock a
response can make a test pass that should not — and it deserves its own
decision rather than arriving inside an introspection phase.

## Phasing

Each phase is a shippable PR.

1. **Surface shell and navigation.** Engine discovery, session actor,
   screencast, input forwarding, surface definition and renderer,
   `browser open|list|close|navigate`, `get url|title`. Produces a usable
   human browser pane.
2. **Automation.** Snapshots and refs, `wait`, `click`/`fill`/`press`/`scroll`,
   `get text|html`, `--json`, `--snapshot-after`, screenshots. This is the
   phase that makes it an *agent* browser.
3. **Introspection.** Console, network, cookies, storage, downloads, viewport,
   `eval`.
4. **Parity polish.** Session restore across app restarts, a default URL
   derived from the workspace's detected listening ports, and remote/PWA
   mirroring.

Phase 4 is three items of very different size, so it ships as three pieces
rather than one:

- **4a, session restore.** A restored surface reopens its page the first time
  it becomes visible. Lazy rather than eager: `suspend_when_hidden` exists so a
  background browser costs nothing, and launching one Chromium per persisted
  tab at startup would spend exactly what that policy saves. A session that
  died while a surface was watching it keeps the manual button — silently
  respawning a browser that just crashed would hide the crash. Frontend only.
  This also fixed a shipped bug: a reopen rebinds `resource_key` in place, the
  workbench does not key its panel on it, and the reused component kept showing
  the old placeholder over the live page.
- **4b, a default URL from the workspace.** Probe the conventional dev-server
  ports on loopback and prefill a new surface's address instead of opening
  blank.
- **4c, remote/PWA mirroring — deferred by decision, not by backlog.** It is
  roughly phase-1 sized: a gateway websocket route, frame transport,
  lease-aware input forwarding, auth and policy integration, and a PWA-side
  surface. That is a large bill for the narrow case of watching a headless
  browser from a phone, and it touches the remote auth surface, which is the
  part of Wardian least improved by being touched speculatively. Tracked as
  #876, with the open questions written down, so the cost is visible when
  someone wants it rather than the item sitting here as if it were nearly done.

Phases 3 and 4 should start by reading
[agent-browser](https://github.com/vercel-labs/agent-browser) rather than from
a blank page. It already ships `network route`, `cookies`, `storage`,
`state save|load`, `profiles`, `auth`, and `tab|window|frame` — most of the two
backlogs — and it solved them in Rust against the same protocol Wardian now
speaks, so its command shapes and failure modes transfer directly. Phases 1 and
2 were designed from the constraints rather than from that source; there is no
reason to repeat that for the rest.

## Risks

- **No Chromium present.** The surface must fail with a named, actionable error
  and an install path, never a blank pane.
- **Screencast cost.** Cap the frame rate and pause on hide;
  `render_policy: "suspend_when_hidden"` already enforces the second half.
- **Security.** An agent-driven browser reaches the user's network. Default to
  an isolated profile with no access to the human's cookies. Bind the CDP
  endpoint to loopback on an ephemeral port and never expose it through the
  remote/PWA server. A persistent profile is opt-in and per-session.
- **Scope.** This is a large feature. Phase 1 on its own is a substantial PR,
  and the phases should not be merged as one.

## Testing

Per the layer boundary table in `AGENTS.md`:

- **Rust unit** — engine discovery ordering, CDP message framing, ref table
  generation and staleness, control request routing, short-ref resolution.
- **Browser E2E** — surface opens, chrome bar behavior, placeholder on an
  unresolvable session, against a mock session client. Automation assertions
  belong a layer up and are marked `test.skip(...) // @native-only`.
- **Native E2E** — the required layer for every real-browser claim: spawn a
  real Chromium, navigate to a fixture served by the harness, snapshot, click,
  assert the resulting URL, capture a screenshot.
- **Screenshot evidence** under `e2e/screenshots/agent-browser-surface/` for
  the PR body.

## Consequences

- **Positive**: closes the agent verification loop for anything that renders.
  An agent can assert what a page does rather than that a build succeeded.
- **Positive**: fills an already-reserved surface slot using the existing
  registry, broker, and control-plane machinery rather than new infrastructure.
- **Positive**: CDP gives Wardian the introspection cmux's WKWebView port
  reports as `not_supported`, and puts Wardian on the same engine as
  agent-browser, whose already-solved surface is the natural reference for
  phases 3 and 4 rather than a from-scratch design.
- **Positive**: one CLI for humans and agents, matching how `wardian agent` and
  `wardian workflow` already work.
- **Negative**: an external Chromium is now a runtime dependency of one
  surface, with discovery and failure modes to own.
- **Negative**: a screencast pane will never feel as immediate as a native
  webview, which is a real cost paid to keep Windows working and the DOM
  composited.
- **Negative**: it is the largest new subsystem since the workflow engine, and
  phases 3 and 4 are plausible candidates for never being finished.

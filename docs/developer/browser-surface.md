# Browser Surface

The browser surface renders a real Chromium page inside the workbench and lets
agents drive it through `wardian browser`. Design rationale is in
[the spec](https://github.com/wardian-app/Wardian/blob/main/docs/specs/2026-08-09-agent-browser-surface.md);
this page is the
working reference.

## Shape

```
wardian browser ─┐
                 ├─► control plane ─► BrowserSessionBroker ─► CDP ─► Chromium
BrowserSurface ──┘                            │
                                              └─► BrowserSessionEvent ─► surface
```

The browser is an out-of-process Chromium, not a Tauri child webview. The
session is a backend-owned runtime resource with the same ownership rules as a
PTY session: surfaces attach as presentations, and detaching a presentation —
closing a tab, unmounting the renderer — never disturbs the runtime.

## Modules

| Path | Responsibility |
| --- | --- |
| `src-tauri/src/state/browser_session/engine.rs` | Find a Chromium, launch it headless with an isolated profile, read its debug endpoint. |
| `src-tauri/src/state/browser_session/cdp.rs` | DevTools Protocol client: correlation, target sessions, event stream. |
| `src-tauri/src/state/browser_session/snapshot.rs` | The injected DOM walker, the ref ledger, and staleness rules. |
| `src-tauri/src/state/browser_session/network.rs` | The request ledger: folding `Network.*` events into bounded records. |
| `src-tauri/src/state/browser_session/discovery.rs` | Guessing what address a workspace is serving, for a session opened with no URL. |
| `src-tauri/src/state/browser_session/actor.rs` | Session lifecycle, page operations, screencast, input. |
| `src-tauri/src/commands/browser.rs` | Tauri commands and the shared operations the control plane calls. |
| `crates/wardian-core/src/browser.rs` | Wire types shared by the app, the control plane, and the CLI. |
| `crates/wardian-cli/src/browser.rs` | `wardian browser`. |
| `src/features/browser/` | The surface and its client. |

## Engine discovery

First hit wins: `WARDIAN_BROWSER_BINARY` → Edge → Chrome → Brave → Chromium on
Windows; Chrome first elsewhere. Launch flags set `--headless=new`,
`--remote-debugging-port=0`, and a `--user-data-dir` under
`<WARDIAN_HOME>/browser/profiles/<browser_id>`, so an agent-driven browser
never inherits the human's cookies or signed-in state. The chosen port is read
from Chromium's `DevToolsActivePort` file, which is deleted before launch so a
crashed predecessor's port cannot be mistaken for this one.

If no engine is found, `browser_engine_status` reports `available: false` with
a message naming the override, and `open` fails with
`browser_engine_unavailable` rather than producing a blank surface.

## Snapshots and refs

`snapshot` injects a walker that stamps `data-wardian-ref="eN"` and
`data-wardian-snapshot="<generation>"` on each element it reports, capped at
400 elements and 160 characters per field. The ledger records both the
generation and what each ref pointed at.

Three checks stand between a ref and a click, and all three are refusals rather
than best guesses:

1. **Generation.** A main-frame `Page.frameNavigated` or
   `Page.navigatedWithinDocument` bumps the generation, so refs do not survive
   a route change even when no frame commits. An iframe navigating does not
   bump it, because it must not throw away refs the agent just took — both
   events are filtered to the main frame, `frameNavigated` by its missing
   `parentId` and `navigatedWithinDocument` by its `frameId`, which is compared
   against the main frame id read from `Page.getFrameTree` at attach and
   refreshed on every main-frame commit. Acting on an older generation returns
   `snapshot_stale`.
2. **Uniqueness.** The action selector pins both the ref and the generation and
   requires exactly one match; several matches return `ref_ambiguous`.
3. **Identity.** The guard re-derives the element's role and accessible name
   and compares them to what the snapshot recorded. A page can recycle a DOM
   node for different content without navigating — a virtualized list does this
   constantly — and the stamped attribute travels with the node. A repurposed
   element returns `ref_changed`.

The role and name derivation is a single shared JS constant (`IDENTITY_JS`)
used by both the walker and the guard. Any drift between the two would produce
spurious `ref_changed` refusals, so they must not be duplicated.

The ledger deliberately retains the previous snapshot generation across an
invalidation so the error is the actionable "stale, re-snapshot" rather than
the misleading "no snapshot has been taken".

A snapshot taken while the page navigates is discarded rather than published:
recording it against a generation that has already moved would hand back refs
that are stale the moment they are returned.

**Dropped events fail closed.** The session's event pump reads a bounded
broadcast channel shared with screencast frames. On `RecvError::Lagged` it
cannot know whether a `Page.frameNavigated` was among the discarded messages,
so it invalidates every outstanding ref and re-reads the page's URL and title.
A spurious `snapshot_stale` costs one re-snapshot; a missed navigation would
let a ref act on a different document.

## Introspection

### The network ledger

`Network.enable` goes on at attach, beside `Page`, `Runtime`, and `Log`. Lazy
enabling on the first `network` call was the tempting alternative: an agent asks
about the network *after* something went wrong, and a ledger that starts
recording at the moment of the question is empty exactly when it matters.

Always-on costs event volume on a channel that also carries screencast frames,
and a lag there is not a dropped frame — the pump cannot know whether a
`Page.frameNavigated` was among the discarded messages, so it invalidates every
outstanding ref. Two things keep that in hand: the CDP event channel is 2048
rather than 512, and the pump's `Network.*` arm performs no protocol calls, so
it cannot stall the loop the way the title read after `Page.loadEventFired`
does.

**The ledger is not cleared by navigation, and that is deliberate.** Console
entries belong to the page that produced them. Network records belong to the
investigation — and the document request that starts a navigation is emitted
*before* the `Page.frameNavigated` that would clear it, so clearing on
navigation would reliably delete the single most interesting record in the
ledger. `network --clear` is explicit instead.

A redirect reuses one request id and reports the hop it just finished as
`redirectResponse`. Each hop therefore becomes its own record, and every later
update resolves to the *newest* record for that id, so a chain does not collapse
into whichever status happened to land last.

Headers are stored with the request; bodies never are. `network <id> --body`
reads one back through `Network.getResponseBody` at call time, capped and cut on
a character boundary. A body outlives its request only while Chromium's own
buffer holds it, so a failed read is an ordinary outcome reported as
`body_error` rather than an empty body.

The ledger trails the page: it is written from the event pump, while a `wait` on
page text returns the moment the DOM changes. Engine-backed tests settle on the
ledger itself rather than on the page, because a test that settles on the page
passes against a ledger that never fills in.

### Cookies and storage

Cookies go through `Network.getCookies` (page-scoped) or `Storage.getCookies`
(`--all`), with `setCookie`, `deleteCookies`, and `clearBrowserCookies` behind
the mutations. A set or delete with neither `--url` nor `--domain` falls back to
the page's own address, because the protocol silently drops a cookie that has
nowhere to live; on `about:blank` there is no address to fall back to and the
call is refused with the fix named.

Storage goes through `Runtime.evaluate` rather than the `DOMStorage` domain: the
page's own origin comes for free, `localStorage` and `sessionStorage` are
identical to address, and no `storageId` plumbing is needed. The one expected
failure is an origin with no web storage at all — `about:blank`, a sandboxed
frame, a `data:` URL — where the DOM throws `SecurityError`. That is translated
into a named refusal pointing at the fix. Both ceilings are applied in the
backend, not in the page: a value that would blow an agent's context should not
be serialized across the protocol first.

### Downloads

Downloads are written to `<WARDIAN_HOME>/browser/downloads/<browser_id>/`, a
**sibling** of the profile directory rather than a child. The profile is deleted
on close, and taking the agent's export with it would defeat the point of
downloading. Growth is bounded at the other end: the broker prunes download
directories older than seven days when it starts, and leaves alone any whose
age it cannot read.

`Browser.setDownloadBehavior` runs in `allowAndName`, which writes each file
under its download GUID — deterministic before the suggested name is known,
which is what makes the file findable at all, and useless to a caller
afterwards. The rename to the suggested filename happens in `downloads()`, not
in the event pump: the pump must stay free of filesystem work, and doing it on
read also cannot race the browser's own finalization of the file. Only the file
*name* of the suggestion is used, so a page suggesting `../../.bashrc` cannot
escape the directory. A rename that fails reports the GUID path rather than
failing the download.

Because these are browser-scoped events carrying no target session, the pump's
session filter admits **all** session-less events: one connection serves exactly
one browser, so an unaddressed event on it is this session's by construction.
Events carrying some *other* target's session id stay filtered out.

### No redaction

`network <id>` prints `Authorization` and `Cookie` headers; `cookies` prints
values. Redacting would defeat the commands — "was the token sent?" is the
question — and redacting in one place while printing in the other would be worse
than doing neither. What makes it defensible is the isolated profile: these are
credentials the session itself acquired, never the human's. Both guides say
plainly that the output does not belong in a shared artifact.

## Where a session opens with no URL

An unspecified address is a question, not an answer. A browser opened in a web
workspace almost never wants `about:blank`, and an agent verifying a change
should not have to guess which port the dev server picked. `open_address`
resolves the three cases — an explicit URL, `--blank`, and a guess — and is
split from the probing so the decision is testable without a socket.

The guess is a heuristic and is treated as one. Reading the OS socket table and
attributing sockets to processes under the workspace would be the precise
answer, but that needs per-platform code, elevated access on some hosts, and a
process-ancestry walk to be even approximately right. Two cheap signals get most
of the value:

1. **What the workspace declares.** `vite.config.*`, `package.json`, and
   `.env*` at the workspace root are scanned — not parsed — for a port.
   Three formats would need three parsers, `vite.config.ts` is TypeScript
   rather than data, and the cost of a wrong guess is one probe against a
   closed port. Only the root is read: a monorepo's hundred `package.json`
   files are a hundred wrong answers, and the root is where the command an
   operator actually runs lives. Ports below 1024 are ignored, because no dev
   server asks for privileges.
2. **What is actually listening.** Declared ports first, then a conventional
   list ordered by what modern web workspaces use. Every candidate is probed at
   once with a 250 ms loopback connect, and the **highest-ranked** listener wins
   rather than whichever socket answered first — a fast reply from a stray
   service should not outrank the port the workspace declared.

The whole sweep is bounded at 24 candidates, and a declaration file is only read
at its head, so opening a tab cannot turn into a lockfile-sized disk read.

Nothing here can be wrong in a damaging way. The worst case is a tab pointed at
the wrong local service, which the address bar fixes. `--blank` says the blank
page was the point.

`wardian browser open` defaults its `--workspace` to the working directory,
because an agent runs it from the workspace whose dev server it wants to look
at. The launcher passes the selected agent's workspace, the same rule the user
terminal already follows; with nothing selected there is no declaration to read
and detection falls back to the conventional ports.

## Error codes

Carried intact from the runtime to the CLI's `--json` output; they are not
flattened to `generic`.

| Code | Meaning |
| --- | --- |
| `browser_not_found` | No session matches the target. |
| `browser_ambiguous` | An id prefix matched several sessions. |
| `browser_engine_unavailable` | No Chromium found, or it would not start. |
| `browser_protocol_error` | The DevTools call failed. |
| `browser_wait_timeout` | A `wait` predicate never became true. |
| `browser_invalid_request` | Bad URL, unknown verb, missing value. |
| `browser_io_error` | A screenshot could not be written. |
| `snapshot_stale` | The ref predates the current page. Re-snapshot. |
| `snapshot_missing` | No snapshot has been taken yet. |
| `ref_detached` | The element left the DOM. |
| `ref_changed` | The element was recycled for different content. |
| `ref_ambiguous` | The ref matched more than one element. |
| `ref_malformed` | Not an `eN` token. |
| `browser_read_only_presentation` | A mirroring surface tried to drive the page. |

## The surface

`BrowserSurface` renders base64 JPEG screencast frames into an `<img>` and
forwards pointer, wheel, and keyboard events as `Input.dispatch*` calls.
Because the image is letterboxed to fit its pane, `pageCoordinates` maps a
client offset back into page space; a click in the letterbox is dropped rather
than clamped to an edge.

Provisioning is transactional. Opening Browser from the launcher creates a
session before the surface exists, so if the store rejects the mutation — a
read-only workbench, a concurrent transaction — the provisioner's `release`
closes the session it just created rather than leaving an unowned Chromium.

The surface's `render_policy` is `suspend_when_hidden`, so a hidden tab stops
the screencast while the page keeps running. An attach that resolves after the
effect was already torn down detaches immediately, or the stream would outlive
the surface that asked for it. `Ctrl`/`Cmd` chords are left to the workbench so
a focused page cannot swallow tab switching or the palette.

**Keys carry a virtual key code.** `key` and `code` describe a keystroke to the
page; `windowsVirtualKeyCode` is what Blink reads to decide what the keystroke
*does*. Without it Backspace deletes nothing, the arrows do not move the caret,
and Enter neither submits nor inserts a newline — while printable characters
keep working, because a `keyDown` carrying `text` synthesises its own character
event. That asymmetry is why the gap reads as "typing works, editing does not".
`browser_session::keys` maps a DOM key identity onto the US-layout code the
protocol's own tooling sends: named keys resolve from `key`, shifted printables
from `code`, and a bare character as a last resort.

**The page is rendered at the size of the pane.** A browser surface observes
its viewport element and pushes the settled size through `set_browser_viewport`
(debounced by `VIEWPORT_SETTLE_MS`, because `Emulation.setDeviceMetricsOverride`
relays out the document). Otherwise the page renders at a fixed 1280×800 and
the frame is rescaled into whatever the pane measures — a resampled screenshot
of a layout nobody chose. Only the driving presentation resizes: the viewport
belongs to the shared page, so a mirror reflowing it would move the page under
the pane that owns it.

**A popup is presented in place of its opener.** A surface has one viewport,
so a window a page opens either replaces what is on it or runs where nobody can
see or drive it. The second is what used to happen: `window.open` and every
`target="_blank"` link — every OAuth flow — created a target the session
ignored, leaving the operator watching an opener that would never change.

The session keeps a stack of attached page targets and presents the last. A
popup is adopted with the domains its opener had, the pane's viewport, and the
screencast moved onto it; `url`, `title`, snapshots, and actions then all
describe the popup, because that is what is presented. `summary.popup` is the
one bit that says the opener is still behind it, and the surface's **Close
popup** button is the way back: the popup's own history has no entry for the
page that opened it, so Back cannot leave it.

Adoption is keyed on the target being new rather than on `openerId`. Chromium
implies `noopener` for `target="_blank"`, so the commonest popup of all arrives
with no opener recorded — and this browser is exclusive to its session, so any
page target that appears after open is one this session's page asked for. The
targets that already existed are recorded at open, because discovery
re-announces everything the moment it is switched on and the browser starts
with a page of its own.

Discovery rather than `Target.setAutoAttach`: auto-attach would also pause
every service worker until this client released it, which is a page-breaking
way to learn about a popup.

**A dialog stops the page, and something always answers it.** `alert`,
`confirm`, `prompt`, and `beforeunload` hold the renderer until a client calls
`Page.handleJavaScriptDialog`. With `Page.enable` on and nobody answering, the
page stopped for good — the surface kept its last frame and every later call
timed out, which reads as a frozen browser rather than as a dialog.

Dialogs have a subscription of their own, separate from the session's event
pump. The pump does page work inline — a `Runtime.evaluate` for a title, an ack
for a frame — so a pump that owned dialogs would deadlock: the one call that
releases the renderer would queue behind a call the renderer cannot answer. The
watcher only reads, and hands answering to a task of its own.

Who answers depends on who could see it. A session with a presentation attached
holds the dialog, reports it in `summary.dialog`, and the surface shows it with
the buttons that dialog kind has. A session nobody is watching answers its own,
and so does a dialog raised by a target that is not being presented — the
opener behind a popup. `beforeunload` is accepted, because the navigation that
raised it was asked for and refusing it would silently cancel the caller's own
request; everything else is dismissed, because a `confirm` nobody saw has not
been agreed to. The last presentation to detach answers whatever was still
waiting, so closing a pane cannot strand a page.

**One driver at a time.** `attach_browser_screencast` mints an opaque lease
token and returns it with `can_drive`. The first attachment holds the lease and
later ones mirror it read-only, the same arrangement the terminal broker uses.

The token, not the presentation id, is the credential. A presentation id is
derived from the surface and session ids, so any caller could guess one; and a
single presentation attaches several times across effect re-runs. Every
surface-originated mutation — pointer, wheel, key, **navigation, and viewport**
— carries the token and is refused with `browser_read_only_presentation`
without it. Omitting the token is a refusal, not a bypass; that is what makes
this an enforcement boundary rather than a frontend convention. The surface
also disables its chrome bar and viewport, because a control that looks live
and silently does nothing is worse than one that is visibly inert.

Detach is keyed on the token, so a cleanup racing a re-attach releases only its
own attachment and never the newer one. The lease passes to the
longest-attached survivor when the holder leaves.

One presentation streams once. A second attach under the same presentation id
supersedes the first and inherits whatever it held, because the earlier
attachment belongs to a presentation that no longer exists — a reloaded
webview, or an effect whose cleanup has not landed yet. Without that, a
remounted surface mirrors a lease that nobody will ever release and the pane is
stuck showing **Read only** over a page it is the only viewer of.

`can_drive` is an answer, not a fact: it describes the lease at the moment of
attach, and the lease moves. `BrowserSessionEvent::Lease` republishes the
holder on every attach and detach so a surface follows the handover — it names
the *presentation id*, never the token, because the event reaches every
listener and the token is the credential that admits input.

Attach and detach hold a per-session transition lock across their CDP
start/stop. The viewer list and the stream have to move together: without it a
second attach can observe a non-empty list and skip a start that then fails and
rolls back, and a detach's `stopScreencast` can land after a concurrent
attach's `startScreencast`, leaving a live attachment with no frames.

The control-plane path passes `None`: `wardian browser` reaches these
operations through the control server, never through a surface, and is not a
competing presentation.

## Lifecycle

A session ends on `wardian browser close`, on its owning agent's termination
(`kill_agent` calls `close_for_agent`), or on app exit (`shutdown_all`).
Closing a tab only detaches the presentation.

Surface state persists `{ url, viewport }`, not the session id — the id lives
in `resource_key` and means nothing after a restart. A restored surface whose
session is gone mints a new one at the persisted URL and rebinds to it.
`rebind_resource` may legitimately answer something other than `allow` — a
concurrent workbench transaction, a read-only document — so reopen closes the
session it just created on both a rejected decision and an exception, the same
rule launcher provisioning follows with its `release`. Either way it *throws*
rather than logging quietly, because an automatic restore has to be able to say
why it gave up instead of leaving a placeholder that looks like it is still
trying.

**When a restore happens by itself.** `shouldAutoRestore` gates it on four
things, and each one is a decision rather than a precaution:

- The session must be missing, which is the whole trigger.
- No `closed` event may have been seen. A session that died while this surface
  was watching it produced one, and silently respawning a browser that just
  crashed would hide the crash. That case keeps the manual Reopen button.
- The surface must be visible. `suspend_when_hidden` exists so a background
  browser costs nothing, and auto-launching one at startup would spend exactly
  what that policy saves. A hidden tab restores the moment someone looks at it.
- The persisted URL must be worth a Chromium process. An empty or `about:`
  address is not; the operator can type one.

It runs **once per surface, ever** — a ref, deliberately not reset when
`resource_key` changes, so a restore that produces a session which immediately
dies cannot restore again in a loop. A failure is shown in the placeholder with
the manual button still live.

**A rebind resets session-scoped state.** The workbench does not key its panel
on `resource_key`, so a reopen reuses this component instance with all of the
previous session's state. Without an explicit reset the pane keeps showing
"Browser session unavailable" over a live page: the mount lookup only ever
*sets* `missing`, and a session that is already loaded emits no further state
event to clear it. The reset runs during render on a changed `resource_key`
rather than in an effect, so there is no frame of stale placeholder.

The persisted viewport is carried into the replacement session rather than
dropped. A restore that reverted to the default size would reopen the page at a
width the operator never chose.

## Testing

| Layer | What it covers | How to run |
| --- | --- | --- |
| Rust unit | URL normalization, wait predicates, ref staleness, snapshot parsing, network event folding, filters, storage and download bounds, CLI arg parsing, error codes | `cargo test --lib browser_session` |
| Engine-backed | Real Edge: navigate, snapshot, fill, click, stale refusal, screenshot, viewport, editing keys, lease handover, popup adoption and return, dialogs held and auto-answered, short refs, the network ledger, cookies, storage, a real download | `cargo test --lib browser_session::tests -- --ignored --test-threads=1` |
| Frontend | Coordinate mapping, input forwarding, read-only mode, lease handover, viewport tracking, the dialog bar and popup controls, screencast attach/detach, reopen path, footer counts | `npx vitest run src/features/browser` |

The engine-backed tests are `#[ignore]`d so a machine without a Chromium still
runs a green suite. They serve a fixture over an ephemeral loopback port rather
than reaching the network.

## Lifecycle failure modes

- **Failed launch.** `open` keeps ownership of the child across the whole
  fallible region so it can `kill().await` — which also reaps — *before*
  removing the profile. `kill_on_drop` alone would terminate without reaping,
  and on Windows a dying Chromium still holds its profile lock, so the removal
  would silently fail and the directory would accumulate.
- **Failed first load.** Treated as a page outcome, not a failed open: the
  session is registered and its browser is running, so returning an error would
  strand a live browser with no handle. The caller sees `load_state: failed`.
- **Crashed browser.** The websocket closing publishes a synthetic
  `Wardian.disconnected` event. Subscribers cannot detect closure by the
  channel ending — the sender lives in the connection they hold alive — so
  without that signal a crashed browser would leave the pump waiting forever
  and the surface would never show its reopen path.

  The pump then *reaps* the session: it removes it from the broker before
  announcing, so `browser list` stops showing it and later commands report
  `browser_not_found` instead of resolving a dead connection.

  Two orderings make that reap reliable. `open` registers the session *before*
  spawning its pump — the other way round, a browser dying in between is reaped
  before it exists, the reap finds nothing, and `open` then inserts a dead
  session with no pump left to remove it. And the pump re-checks
  `connection.is_closed()` once before its first `recv`, because a disconnect
  between `subscribe` and that `recv` published to nobody.

  Every teardown path — the reaper, `close`, `close_for_agent` — goes through
  one atomic `take_session`. Whoever gets the session back owns announcing and
  shutting down, so a crash racing an explicit close produces exactly one closed
  event rather than two contradictory ones.

  The connection also latches closed, and calls made afterwards fail
  immediately rather than each waiting out the 30-second call ceiling. Without
  that latch, teardown alone would block for half a minute. The latch check
  lives inside the same `pending` mutex the reader drains under: checking it
  outside leaves a window where a disconnect drains the map and the call then
  registers a sender nobody will ever answer, which — with the screencast
  transition lock held across the call — would stall every later attach and
  detach for the full ceiling.

- **A CLI open the frontend has not surfaced yet.** The control endpoint serves
  before the webview mounts, and a reload can retire the event listener at any
  moment, so the emitted event is never treated as proof of delivery. Every
  non-detached open is recorded first, then emitted, and the record stays until
  a frontend calls `ack_browser_surface_open` for it.

  Reading `pending_browser_surface_opens` does not consume: a frontend that
  reads and then dies before opening anything must not take the work with it.
  Nothing here depends on a message arriving at a particular moment — not the
  event, not a registration, not a release on teardown — which is what earlier
  designs kept getting wrong. Each of those had a window where the backend
  believed a listener existed while none did, and the open in that window
  reached neither the listener nor the queue.

  The frontend does not subscribe until the durable workbench document has
  loaded. Opening into the provisional document would acknowledge the request
  and then lose it, because loading replaces the working document outright.
  Waiting costs nothing precisely because the record survives until it is
  acknowledged.

  Repeat delivery is the accepted cost. The surface is `focus_resource`, so a
  session reported by both the event and the read is focused rather than opened
  twice, and re-reading an unacknowledged open at the next mount is the
  behavior that makes the loss impossible. Acknowledgement is also what keeps a
  remount from replaying work already done: an open the user has seen is gone
  from the record. Sessions that have since closed are pruned on read, and
  `MAX_PENDING_SURFACE_OPENS` bounds a burst nothing ever acknowledges.

- **A session that closes while the surface is mounting.** `getBrowserSession`
  and `subscribeBrowserSession` are both in flight at mount, so a closure in
  between emits its one `closed` event to nobody. The surface re-checks the
  session once its listener is installed: without that the pane stays resolved
  with no frame, no lease, and no Reopen, because the attach failure it would
  otherwise notice is deliberately silent — that handler defers to the very
  `closed` event that was missed.

## Not built yet

Phase 4: session restore across app restarts, remote/PWA mirroring, and a
default URL from the workspace's detected listening ports.

Request interception (`network route`, `--abort`, response mocking) and HAR
recording are out of scope by decision rather than by backlog.
[agent-browser](https://github.com/vercel-labs/agent-browser) ships both and
they are genuinely useful, but they change what the page does rather than
observe it. An agent that can silently mock a response can make a test pass that
should not, which is a different risk class and deserves its own decision.

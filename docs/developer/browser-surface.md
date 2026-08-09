# Browser Surface

The browser surface renders a real Chromium page inside the workbench and lets
agents drive it through `wardian browser`. Design rationale is in
[the spec](../specs/2026-08-09-agent-browser-surface.md); this page is the
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

1. **Generation.** A main-frame `Page.frameNavigated` or a
   `Page.navigatedWithinDocument` bumps the generation, so refs do not survive
   a route change even when no frame commits. An iframe navigating does not
   bump it, because it must not throw away refs the agent just took. Acting on
   an older generation returns `snapshot_stale`.
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
session is gone shows "Browser session unavailable" with a Reopen action that
mints a new session at the persisted URL and rebinds the surface. `rebind_resource`
may legitimately answer something other than `allow` — a concurrent workbench
transaction, a read-only document — so reopen closes the session it just created
on both a rejected decision and an exception, the same rule launcher
provisioning follows with its `release`.

## Testing

| Layer | What it covers | How to run |
| --- | --- | --- |
| Rust unit | URL normalization, wait predicates, ref staleness, snapshot parsing, CLI arg parsing, error codes | `cargo test --lib browser_session` |
| Engine-backed | Real Edge: navigate, snapshot, fill, click, stale refusal, screenshot, viewport, short refs | `cargo test --lib browser_session::tests -- --ignored --test-threads=1` |
| Frontend | Coordinate mapping, input forwarding, read-only mode, screencast attach/detach, reopen path | `npx vitest run src/features/browser` |

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

- **CLI open during startup.** The control endpoint serves before the webview
  mounts, so a `wardian browser open` can win that race and its one-shot event
  reaches nobody. Non-detached opens are queued as well as emitted, and the
  frontend drains the queue right after subscribing. The surface is
  `focus_resource`, so a session reported by both paths is focused rather than
  opened twice, and a queued session that has since closed is dropped.

  Queueing is tied to whether a listener is registered *right now*, not to
  whether one ever was. `register_browser_surface_listener` drains and returns
  an epoch; the effect cleanup passes that epoch to
  `release_browser_surface_listener`, and queueing resumes. Tying it to the
  first drain instead would leave every later gap — a webview reload, an effect
  re-subscription — silently lossy, which is the failure the queue exists to
  prevent. A release for a superseded epoch is ignored, because React can mount
  the replacement listener before the outgoing one's cleanup reaches the
  backend. While a listener is registered nothing is queued, so ordinary agent
  use cannot grow the list, and a burst arriving with nothing subscribed is
  bounded by `MAX_PENDING_SURFACE_OPENS`.

- **A session that closes while the surface is mounting.** `getBrowserSession`
  and `subscribeBrowserSession` are both in flight at mount, so a closure in
  between emits its one `closed` event to nobody. The surface re-checks the
  session once its listener is installed: without that the pane stays resolved
  with no frame, no lease, and no Reopen, because the attach failure it would
  otherwise notice is deliberately silent — that handler defers to the very
  `closed` event that was missed.

## Not built yet

Phase 3 (cookies, storage, network, downloads as first-class commands) and
phase 4 (session restore across app restarts, remote/PWA mirroring, a default
URL from detected listening ports). `console` and `eval` shipped early because
they cost almost nothing on top of the existing protocol client.

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

`snapshot` injects a walker that stamps `data-wardian-ref="eN"` on each element
it reports, capped at 400 elements and 160 characters per field. The session's
`SnapshotLedger` records which generation minted the refs. A main-frame
`Page.frameNavigated` bumps the generation; an iframe navigating does not,
because it must not throw away refs the agent just took.

Acting on a ref from an older generation returns `snapshot_stale`. The ledger
deliberately retains the previous snapshot generation across an invalidation so
the error is the actionable "stale, re-snapshot" rather than the misleading
"no snapshot has been taken".

A snapshot taken while the page navigates is discarded rather than published:
recording it against a generation that has already moved would hand back refs
that are stale the moment they are returned.

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
| `ref_malformed` | Not an `eN` token. |

## The surface

`BrowserSurface` renders base64 JPEG screencast frames into an `<img>` and
forwards pointer, wheel, and keyboard events as `Input.dispatch*` calls.
Because the image is letterboxed to fit its pane, `pageCoordinates` maps a
client offset back into page space; a click in the letterbox is dropped rather
than clamped to an edge.

The surface's `render_policy` is `suspend_when_hidden`, so a hidden tab stops
the screencast while the page keeps running. `Ctrl`/`Cmd` chords are left to
the workbench so a focused page cannot swallow tab switching or the palette.

## Lifecycle

A session ends on `wardian browser close`, on its owning agent's termination
(`kill_agent` calls `close_for_agent`), or on app exit (`shutdown_all`).
Closing a tab only detaches the presentation.

Surface state persists `{ url, viewport }`, not the session id — the id lives
in `resource_key` and means nothing after a restart. A restored surface whose
session is gone shows "Browser session unavailable" with a Reopen action that
mints a new session at the persisted URL and rebinds the surface.

## Testing

| Layer | What it covers | How to run |
| --- | --- | --- |
| Rust unit | URL normalization, wait predicates, ref staleness, snapshot parsing, CLI arg parsing, error codes | `cargo test --lib browser_session` |
| Engine-backed | Real Edge: navigate, snapshot, fill, click, stale refusal, screenshot, viewport, short refs | `cargo test --lib browser_session::tests -- --ignored --test-threads=1` |
| Frontend | Coordinate mapping, input forwarding, read-only mode, screencast attach/detach, reopen path | `npx vitest run src/features/browser` |

The engine-backed tests are `#[ignore]`d so a machine without a Chromium still
runs a green suite. They serve a fixture over an ephemeral loopback port rather
than reaching the network.

## Not built yet

Phase 3 (cookies, storage, network, downloads as first-class commands) and
phase 4 (session restore across app restarts, remote/PWA mirroring, a default
URL from detected listening ports). `console` and `eval` shipped early because
they cost almost nothing on top of the existing protocol client.

# Remote PWA Startup Performance

## Problem

The remote PWA is responsive after it finishes loading, but its first load can
pause before the watchlist appears. The watchlist does not require the desktop
workbench or the terminal and file-rendering surfaces, so those dependencies
must not delay the first remote paint.

## Diagnosis

The production profile used a fresh Playwright browser context per sample,
blocked the service worker, served the built assets from the local Vite
preview server, and fulfilled the remote API requests with deterministic empty
data. The primary metric was first contentful paint (FCP); the correctness
floor was a visible remote watchlist shell and a passing remote PWA smoke test.

Before the change, `/remote` loaded a 1.18 MB decoded entry chunk plus a
937.7 KB generic vendor chunk and terminal-related chunks on the initial path.
The first profile's compressed script requests totalled about 816 KB. The
steady-state FCP samples were approximately 76–88 ms locally, with one cold
sample at 384 ms.

The source cause was twofold:

- `src/main.tsx` statically imported both the desktop `App` and the remote
  mobile app, so the remote route inherited the desktop module graph.
- The catch-all `vendor` manual chunk grouped dependencies used only by
  deferred surfaces, keeping them in the remote startup request set.

## Change

- Keep the small remote shell in the entry path and defer the desktop `App`.
- Defer remote detail, pairing, settings, and inbox components with
  `React.lazy` and a consistent loading fallback.
- Let unclassified dependencies follow Rollup's graph partitioning instead of
  forcing them into a catch-all vendor chunk.

The watchlist remains synchronously available after the shell loads. Opening
agent detail still loads the terminal/chat route on demand, and opening Inbox
or Settings loads only that selected surface.

## Verification

After the change, five fresh local production samples loaded only the React
runtime, remote shell, icons, and bootstrap for the watchlist: about 89.5 KB
compressed in total. No terminal, markdown, graph, PDF, or desktop chunks were
requested. FCP was 32–44 ms across those samples.

The remote browser smoke test passed, and the focused remote/service-worker
unit suite passed 98 tests.

The profile is a local asset/evaluation measurement. It does not represent
tailnet latency, TLS handshake time, gateway response time, or a particular
phone's CPU. Those dimensions should be measured separately if a real device
still shows a delay after the client-side split.

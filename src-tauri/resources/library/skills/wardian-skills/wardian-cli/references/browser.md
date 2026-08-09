# Browser Surfaces

`wardian browser` drives a real Chromium page that also appears as a workbench
surface, so a human can watch what you are doing. Use it to verify anything
that renders: a dev-server route you just changed, a page a bug report names,
a docs page you need to read.

Requires the desktop app running with the same `WARDIAN_HOME`, and a
Chromium-based browser installed (Edge, Chrome, Brave, or Chromium). Override
the binary with `WARDIAN_BROWSER_BINARY`.

## The loop

```bash
wardian browser --json open http://localhost:5173
wardian browser browser:1 wait --load-state complete --timeout-ms 15000
wardian browser browser:1 snapshot --interactive
wardian browser browser:1 fill e3 "wardian"
wardian browser browser:1 click e5 --snapshot-after --json
wardian browser browser:1 get text "#result"
```

Open once, keep the session for the whole task, and close it when done.

## Addressing a session

Sessions are addressed by short ref (`browser:1`) or by id; an unambiguous id
prefix also resolves. `wardian browser list` shows what is open. Opening from a
Wardian-managed terminal attributes the session to you through
`WARDIAN_SESSION_ID`, so it closes when you do. Pass `--detached` to skip
opening a workbench surface, and `--agent` to attribute it elsewhere.

## Refs go stale — that is a feature

`snapshot` mints refs (`e1`, `e2`, …) bound to the page's current generation.
Any main-frame navigation invalidates them. Acting on a stale ref fails with
`snapshot_stale` rather than clicking whatever now sits in that position.

When you see `snapshot_stale`, re-snapshot and use the new refs. Do not retry
the same ref. `--snapshot-after` folds the re-snapshot into the action that
caused the navigation, which is usually what you want after a click.

A ref is checked three ways before it is acted on: the snapshot generation,
that it resolves to exactly one element, and that the element is still what the
snapshot described. Any of them failing is a refusal, never a best guess.

Other ref failures: `snapshot_missing` (no snapshot taken yet),
`ref_detached` (the element left the DOM), `ref_changed` (the element was
recycled for different content, as virtualized lists do), `ref_ambiguous` (the
ref matched several elements), `ref_malformed` (not an `eN` token). All of them
mean the same thing for you: re-snapshot.

## Commands

| Command | Purpose |
| --- | --- |
| `browser open [url] [--agent] [--workspace] [--width --height] [--detached]` | Start a session. A bare host is treated as `http`. |
| `browser list` | Open sessions, one line each. |
| `browser <target> close` | End the session and stop its browser. |
| `browser <target> navigate <url\|back\|forward\|reload\|stop>` | Move the page. |
| `browser <target> get url\|title\|text\|html [selector]` | Read the page, optionally scoped to a CSS selector. |
| `browser <target> wait --load-state\|--selector\|--text\|--url-contains\|--function [--timeout-ms]` | Block until a condition holds. Exactly one condition. |
| `browser <target> snapshot [--interactive]` | Mint refs. `--interactive` returns only what actions can target. |
| `browser <target> click\|hover\|scroll <ref> [--snapshot-after]` | Act on a ref. |
| `browser <target> fill\|press\|select <ref> <value> [--snapshot-after]` | Act on a ref with a value. |
| `browser <target> screenshot <path> [--full-page]` | Write a PNG. |
| `browser <target> viewport <width> <height>\|reset` | Resize the rendered page. |
| `browser <target> eval <expression>` | Evaluate an expression and print its JSON value. |
| `browser <target> console [--level error\|warning\|info] [--clear]` | Console messages since the last navigation. |
| `browser <target> network [--filter] [--method] [--status] [--type] [--failed] [--limit] [--clear]` | Requests the page made. |
| `browser <target> network <request-id> [--body]` | One request in full, headers both ways. |
| `browser <target> cookies [--all]` | Cookies in this session's profile. |
| `browser <target> cookies set\|delete <name> [...]`, `cookies clear` | Change them. |
| `browser <target> storage local\|session [key]` | Read web storage at the page's origin. |
| `browser <target> storage local\|session set\|remove\|clear [...]` | Change it. |
| `browser <target> downloads [--clear]` | Files this session downloaded, with resolved paths. |

## Seeing what the page did

The DOM tells you what a page *shows*. When a submit silently 500s, the DOM,
the snapshot, and the screenshot all look exactly like success — the evidence
is in the request. Reach for `network` before you reach for `eval`.

```bash
wardian browser browser:1 network --failed          # what went wrong
wardian browser browser:1 network --filter /api/ --limit 20
wardian browser browser:1 network 12345.7 --body    # one request in full
```

The ledger records every request from the moment the session opened and is
**not** cleared by navigation, so the document request for a page load is in
there too. `--clear` is the only thing that empties it. Records are capped at
500; `--limit` keeps the most recent.

The ledger is filled by protocol events, so it trails the page by a moment.
After an action, `wait` for the outcome before reading it — the same habit that
keeps snapshots honest.

`downloads` resolves each completed file to a real path under its suggested
name, and those files outlive the session: closing a session removes its
profile, never its downloads.

**This output carries secrets.** `network <id>` prints `Authorization` and
`Cookie` headers and `cookies` prints cookie values, because a redacted answer
would not answer the question being asked. They are this session's own
credentials, never the human's — the profile is isolated — but do not paste
that output into a PR, an artifact, or anywhere else it outlives the task.

## Habits that keep this reliable

- `wait` before `snapshot`. A snapshot of a half-loaded page mints refs that
  the next paint invalidates.
- Prefer `snapshot --interactive` over a full snapshot. Snapshots are capped at
  400 elements, and the full walk spends that budget on text nodes.
- Prefer `get text "<selector>"` over `get html` when checking an outcome. HTML
  for a real page is large and mostly irrelevant to what you are verifying.
- Use `--json` when you intend to branch on the result. Error codes are stable;
  prose is not. The flag goes after `browser` (`wardian browser --json list`) or
  at the end of a target call (`wardian browser browser:1 get url --json`);
  `wardian --json browser …` is not accepted.
- The profile is isolated per session, so nothing is signed in. Do not assume
  the user's cookies or logins are available.
- `javascript:`, `data:`, and `file:` URLs are refused.

## Reporting what you saw

A browser session is evidence. When you use one to verify a change, say what
you loaded, what you did, and what the page showed — a screenshot path or the
text you read — rather than reporting only that the command succeeded.

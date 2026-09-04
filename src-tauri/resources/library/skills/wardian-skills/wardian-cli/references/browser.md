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
wardian browser --json open               # lands on the workspace's dev server
wardian browser browser:1 wait --load-state complete --timeout-ms 15000
wardian browser browser:1 snapshot --interactive
wardian browser browser:1 fill e3 "wardian"
wardian browser browser:1 click e5 --snapshot-after --json
wardian browser browser:1 get text "#result"
```

Open once, keep the session for the whole task, and close it when done.

`open` with no URL looks for the workspace's dev server — the port declared in
`vite.config.*`, `package.json`, or `.env`, then the ports frameworks commonly
use — and loads the first one that is listening. It defaults `--workspace` to
your working directory, so running it from the workspace you are changing is
usually enough. Pass a URL to be explicit, or `--blank` for no page at all.
Check the address you actually landed on before treating the page as evidence.

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

Discover the action needed without loading a full command catalogue:

```bash
wardian schema browser '<target>'
wardian schema browser '<target>' network
```

Use the literal quoted `'<target>'` in schema requests; use `browser:1` or a
session ID in actual actions. Schema discovery does not open a browser or read
Wardian state. Browser action output defaults to text; `--json` selects compact
JSON without changing response fields/types.

`wait` accepts exactly one condition. `eval` and `wait --function` execute page
JavaScript; expressions can have side effects. Clicks, navigation, cookie and
storage writes can change application state. `screenshot` writes a file;
ledger `--clear` options discard inspection evidence.

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

- Wait for the relevant page state before `snapshot`. Navigation invalidates
  the generation; DOM changes can detach or change a ref even without navigation.
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

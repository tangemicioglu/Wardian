# Terminal background resynchronization

## Status

Implemented and verified with frontend unit coverage plus a native mock-PTY
regression.

## Problem

When the desktop window has been in the background for a long time, its
WebView can return to the foreground with a large retained terminal-event
backlog. Draining that backlog through every mounted renderer can monopolize
the UI thread and make the application appear frozen.

This is distinct from switching between Workbench tabs. Hidden resident
terminals inside the app deliberately stay subscribed so their normal reveal
path does not require replay or reconstruction.

## Scope

This change applies only to a real desktop application background/foreground
transition. It changes frontend broker consumption; it does not pause a PTY,
change provider output retention, alter terminal ownership, or change renderer
residency.

## Invariants

1. Backgrounding stops new frontend terminal-event drains, but native PTYs and
   the backend event log continue without interruption.
2. Foregrounding obtains one authoritative broker snapshot and treats its
   `sequence_barrier` as the new consumer cursor before allowing live event
   drains.
3. The snapshot is applied to every currently bound presentation and is
   acknowledged before events after the barrier are consumed.
4. A background/foreground transition must not invoke terminal resize,
   viewport-report, owner-resync, renderer-fit, or WebGL lifecycle operations.
   Existing geometry and reveal paths remain their sole authority.
5. On a failed foreground snapshot, the client remains paused rather than
   replaying an unbounded stale backlog. A later foreground/visible request can
   retry safely.
6. A runtime-generation transition continues to use the existing
   generation-aware subscription recovery path.

## Design

`TerminalSessionClient` gains an application-visibility state shared by its
registered instances.

- When the app becomes backgrounded, a client records that a snapshot barrier
  is required and rejects new drain scheduling. A drain already applying one
  bounded batch may finish, but it schedules no later batch.
- When the app returns, clients with a visible mounted presentation are resumed
  one at a time. Each client requests a normal terminal snapshot, applies it to
  all of its presentation bindings, advances and acknowledges its shared
  cursor to the snapshot barrier, and only then drains events after that
  barrier.
- Clients without a visible mounted presentation remain paused. Their normal
  presentation-visible path asks for the same resumption before it reveals or
  processes new output. This avoids snapshotting every off-screen terminal at
  once.
- Native Tauri focus changes are the primary application signal. DOM
  `visibilitychange`, `focus`, and `blur` are fallbacks and are made
  idempotent with the native signal.

The new barrier resynchronization is intentionally separate from
`requestPresentationSnapshot` and owner resynchronization. The former is
renderer-specific and must not move the shared stream cursor; the latter has
lease and geometry semantics that application visibility must not reuse.

## Non-goals

- Do not change the native replay limit or snapshot payload shape in this
  patch.
- Do not resize a PTY or fit an xterm renderer on foreground.
- Do not change the resident-renderer/WebGL budget policy.
- Do not treat an internal Dockview tab becoming hidden as application
  backgrounding.

## Verification

Frontend tests must prove that a background client does not read retained
events, foregrounding applies and acknowledges the snapshot barrier before
reading again, failed resumption remains paused, and the new path never calls
geometry or owner-resynchronization IPCs.

Native runtime coverage must run a mock-provider terminal through the
background-resume control path and verify that content remains coherent while
the native resize count and terminal grid geometry do not change solely due to
that transition.

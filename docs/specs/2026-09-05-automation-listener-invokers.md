# Automation Listener Invokers (file, webhook, web poll)

- **Status:** Implemented
- **Date:** 2026-09-05
- **Issues:** #22 (diverse trigger conditions), #60 (external webhook trigger)
- **Builds on:** [Trigger / Invoker Foundation](./2026-05-30-workflow-invoker-foundation.md), [Schedule Invoker](./2026-05-30-workflow-schedule-invoker.md)
- **Deliberately not:** #391 (lifecycle hooks around a *running* automation), #1008 (general run concurrency contract)

## Context and Problem Statement

An automation run enters the engine through an **invoker**, which supplies
`{ blueprint, input, bindings, provider, workspace, assignments }`. The blueprint
declares behavior; the invoker decides when and with what context a run starts.
This boundary is already settled: `registry.rs` carries a regression test
asserting `file_watcher` and `scheduled_trigger` are *not* node types, and
`docs/automations/triggers.md` names file and webhook launches as future
invokers on the same rails.

Two invoker families ship today:

| Family | Config | Arming | Launch |
| --- | --- | --- | --- |
| Schedule | `library/schedules.json` | 5s tick loop | `automation/schedule.rs` |
| Session close | `library/session-close-invokers.json` | conversation-boundary event | `automation/session_close.rs` |

Nothing can start a run from an **external event**. A user who wants "run this
automation when that file changes", "…when CI posts to this URL", or "…when a
new version of that dependency is released" has no mechanism.

Two structural problems block simply adding three more families:

1. **Launch duplication.** `schedule.rs` and `session_close.rs` each
   independently do resolve-blueprint → validate → resolve provider/workspace →
   normalize assignments → build agent catalog → prepare run → drive run → emit
   Inbox update. That is ~120 lines duplicated twice. Three more copies makes
   five, and a fix to one silently misses four. This change stops the growth at
   two by giving the three new variants one shared path; porting the two
   existing families onto it is deliberately left as its own change (see
   Consequences).
2. **Family proliferation.** Each family currently costs a JSON file, a core
   module, an app module, a CLI subcommand tree, a Tauri command set, a Zustand
   store, and an editor. Three more families multiply a fixed cost by three for
   what is one concept: *a persistent listener that watches something and fires
   a run*.

## Proposed Decision

Add **one** new invoker family — the **listener** — whose trigger is a tagged
enum. File watch, inbound webhook, and outbound web poll are three variants of
that one family, sharing storage, CLI surface, commands, UI, and launch path. A
fourth listener type later costs an enum variant, not a subsystem.

### Components and dependency direction

```
crates/wardian-core/src/listeners/       (pure: no Tauri, no net, no watching)
  mod.rs        AutomationListener, ListenerTrigger, load/mutate/save, validation
  file.rs       glob + event-kind matching, watch-root safety rules
  poll.rs       fingerprint extraction, due-poll planning, backoff math
  webhook.rs    path-segment validation, signature verification input shaping
  secrets.rs    listener-secrets.json read/write (separate file, see Trust)
        ^
src-tauri/src/automation/listener/       (effects)
  mod.rs        supervisor: reconcile desired config -> live arming
  launch.rs     the shared invoker launch path for all three variants
  file.rs       notify watcher, debounce accumulator, self-trigger containment
  poll.rs       reqwest polling on the scheduler tick cadence
  webhook.rs    axum listener server, auth, delivery -> run
        ^
src-tauri/src/commands/automation.rs  +  crates/wardian-cli
```

Core never depends on the app. The CLI writes listener config through core, so
`wardian automation listener add` works with the app closed; the app arms it on
its next reconcile.

### Authoritative state and ownership

Config: `<wardian-home>/library/listeners.json`, using the flock-guarded
read-modify-write (`mutate_listeners`) that `session_close.rs` established —
atomic replacement alone prevents torn JSON, not lost updates across the app and
CLI processes.

```rust
pub struct AutomationListener {
    pub id: String,
    pub blueprint_id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger: ListenerTrigger,
    // invocation context, identical in meaning to AutomationSchedule's
    pub provider: Option<String>,
    pub workspace: Option<String>,
    pub input: serde_json::Value,
    pub bindings: HashMap<String, String>,
    pub assignments: AutomationAssignments,
    pub overlap: Option<OverlapPolicy>,    // unset resolves per variant
    pub runtime: ListenerRuntime,         // app-owned, see below
}

/// Every app-written field, in one place, so the write-back merge is one field
/// copy and no config field ever has a second writer.
pub struct ListenerRuntime {
    pub armed: bool,
    pub arm_error: Option<String>,
    pub last_fire_epoch_ms: Option<u64>,
    pub last_run_status: Option<String>,
    pub last_run_error: Option<String>,
    pub last_rejection: Option<ListenerRejection>,  // why a delivery was refused
    pub fire_count: u64,
    pub disabled_reason: Option<String>,   // set when the rate ceiling trips
    // web poll only
    pub poll_fingerprint: Option<String>,
    pub next_poll_epoch_ms: Option<u64>,
    pub consecutive_failures: u32,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListenerTrigger {
    FileWatch(FileWatchTrigger),
    Webhook(WebhookTrigger),
    WebPoll(WebPollTrigger),
}
```

**Ownership rule:** config fields belong to whoever wrote them (user, CLI, UI);
everything under `runtime` belongs to the app. The app writes back using the
same merge discipline as `schedule::persist_runtime` — re-read fresh, copy only
`runtime` — so a concurrent CLI config edit is never clobbered by a run
finishing.

Two consequences of taking that rule literally:

- **`enabled` has exactly one writer, the user.** The rate ceiling does *not*
  flip it. Auto-disable sets `runtime.disabled_reason`, and the effective test
  is `enabled && disabled_reason.is_none()`. Re-enabling from the CLI or UI
  clears `disabled_reason` explicitly. Had the app written `enabled`, a user
  re-enabling a listener concurrently with the ceiling tripping would race, and
  whoever lost would leave no trace of why.
- **Poll change-detection state (`poll_fingerprint`, `next_poll_epoch_ms`,
  `consecutive_failures`) lives in `runtime`, not in `WebPollTrigger`.** It is
  app-written, so putting it in the user-owned config struct would reintroduce
  exactly the dual-writer problem the rule exists to prevent.

### The supervisor

One component reconciles desired state (the config file) against live state
(notify watchers, axum routes, poll timers), reacting to a `listeners-updated`
signal and re-running on the existing scheduler tick.

A per-listener supervisor is not viable: the listener set is mutable at runtime
from two processes, and only a single reconciler can guarantee both "no orphaned
watcher" and "no listener silently unarmed". This mirrors `start_scheduler`,
which aborts and replaces its handle rather than accreting loops.

**Reconcile is gated on a config fingerprint, never on a runtime write.** Every
fire writes `runtime` back to the same file. If reconcile keyed on file change
alone, each fire would tear down and re-arm every watcher — a listener would
disarm itself by working. The supervisor therefore hashes only the
config-relevant fields across all listeners and skips re-arming when that hash
is unchanged, so runtime write-back is invisible to arming. The `listeners-updated`
UI event still fires on runtime writes, because the UI does want to see the new
last-fire time; the supervisor and the UI simply consume that signal
differently.

Arming is **per domain**, so failures are contained: a webhook port conflict
leaves file and poll listeners running.

### Trust boundaries

Three different threat models under one family — this is the load-bearing part
of the design.

**File listener.** Trusts the local filesystem. The threat is not authenticity;
it is *amplification and self-triggering*.

**Web poll listener.** Wardian is the client, so there is no inbound exposure.
The response body is untrusted external data that becomes `trigger.output` and
therefore reaches an agent prompt.

**Webhook listener.** An anonymous network peer until it proves a shared secret.
Authenticate before parsing, cap the body before reading, compare in constant
time, and never enumerate configured paths in a 404.

For both web variants the payload reaches an agent prompt, which is a
prompt-injection surface. Containment is bounded and honest rather than clever:
payloads are size-capped, they stay in `trigger.output.*` as *data* that
interpolation reads (never spliced into the blueprint graph), and the docs say
plainly that a listener payload is attacker-influenced input. A blueprint that
pipes `{{trigger.output.body}}` into a shell node is the author's decision, and
the node reference already carries that warning for `shell`.

**Secrets at rest.** Webhook secrets and poll request headers (which may carry
`Authorization`) are credentials. They live in a **separate**
`<wardian-home>/library/listener-secrets.json`, keyed by listener id, so
`listeners.json` stays safe to inspect, print in CLI output, render in the UI,
and paste into an issue. HMAC verification needs the raw secret, so hashing is
not available for that variant; the separation is what makes the config file
non-sensitive. This matches the existing posture — `remote/storage.rs` keeps
device credentials in files under `<wardian-home>` — while narrowing the blast
radius of showing a listener to a user.

### Self-trigger containment (file listener)

The sharpest failure mode: a run writes into a watched tree and retriggers
itself forever, spawning agent sessions until the machine dies.

Static, fail-closed, at config time in `validate_listener` so CLI and UI both
inherit it:

- reject a watch root that is, contains, or is contained by `<wardian-home>`;
- reject a filesystem root, a drive root, or the OS user home itself;
- default ignore globs (`.git/`, `node_modules/`, `target/`, `dist/`, `.venv/`)
  merged under any user-supplied ignores.

Static checks cannot catch every loop — a run writing into its own workspace is
both legitimate and self-triggering — so a **runtime rate ceiling** is the
backstop: a listener exceeding N fires in a rolling window is auto-disabled with
a durable `disabled_reason` and a visible Inbox surface, rather than being
silently throttled. Auto-disable is the correct failure direction here: a
runaway listener costs real provider tokens.

The ceiling is **cross-cutting, not file-specific**. It is equally the bound on
a webhook listener under `parallel` overlap receiving a flood of distinct
deliveries, which is the one case where `parallel` could otherwise fan out
without limit.

### Watcher overflow (file listener)

`topology_watch.rs` proves the `notify` pattern for one non-recursive directory.
A *recursive* watch on a large tree is a different load profile: on Windows,
`ReadDirectoryChangesW` can overflow its buffer, and `notify` then reports a
rescan rather than the individual paths. Assuming clean per-path events would
silently drop changes under exactly the load where the listener matters most.

An overflow or rescan event is therefore treated as a real fire with unknown
paths: `trigger.output.paths` is empty and `trigger.output.rescan` is `true`, so
a blueprint can tell "these three files changed" from "something under here
changed, go look". Dropping the event, or fabricating a path list, would both be
worse than saying so.

### Overlap and ordering

#1008 owns the general concurrency contract. This design must not pre-empt it,
but a file listener without overlap control is unusable, so it takes the minimum
that is forward-compatible with that vocabulary:

- **Debounce** (default 500 ms): collapse an event burst into one fire,
  accumulating the changed-path set.
- **`overlap`**, per listener: `skip`, `coalesce` (at most **one** pending fire,
  so a burst cannot grow an unbounded queue), `parallel`.

The names match #1008's proposed policies deliberately, so the general contract
can absorb these rather than compete with them.

**Defaults differ by variant, because the events differ.** File events in a
burst describe one logical change, so `skip` is right. Webhook deliveries are
independent events carrying distinct payloads — `skip` would silently drop real
deliveries — so the default is `parallel`. Retry storms do not make `parallel`
unbounded, because idempotency already collapses a retried delivery onto the
same run; only genuinely distinct rapid deliveries fan out, and the rate ceiling
below is the backstop for that. Poll defaults to `skip`: a change detected while
the previous run is still working should not stack.

### Idempotency

`session_close.rs` established the pattern: a deterministic run id from stable
event identity plus an flock claim, so a replay is the same run rather than a
second one. Each variant supplies its own event identity:

| Variant | Event identity | Property this buys |
| --- | --- | --- |
| File | `sha256(listener_id, window_end_ms, sorted_changed_paths)` | a replayed burst is one run |
| Webhook | `sha256(listener_id, delivery_id)` — `X-Wardian-Delivery`, else `X-GitHub-Delivery`, else body hash | a retrying sender does not double-run |
| Poll | `sha256(listener_id, new_fingerprint)` | the same fingerprint never runs twice |

The poll case is the neat one: its idempotency key *is* its change-detection
mechanism, so "fire on change" and "never run twice" are one property.

**The webhook response contract follows from this.** The handler replies `202
Accepted` once the run is durably claimed and prepared, *before* it executes.
Replying after completion would hold the sender open for the length of an agent
run and guarantee a timeout-and-retry; replying before the claim would ack an
event that might never become a run. A retry that lands on an already-claimed
delivery also gets `202`, not a conflict, because from the sender's side the
delivery *was* accepted — returning an error there would drive an infinite
retry loop for a request Wardian handled correctly.

### Fingerprinting (web poll)

`fingerprint` selects what counts as a change:

- `etag_or_last_modified` (default) — cheapest; `HEAD` where the server allows,
  falling back to a conditional `GET`;
- `body_hash` — SHA-256 of the capped body;
- `json_pointer` — RFC 6901 pointer into a JSON body, e.g. `/0/tag_name` against
  `https://api.github.com/repos/<owner>/<repo>/releases`, which is exactly the
  "notify me when they cut a release" case;
- `regex` — first capture group against a text body.

`interval_seconds` has a floor of 30 s. Failures back off exponentially to a
ceiling and record `consecutive_failures`; a flaky endpoint is not a loop, so it
surfaces but never auto-disables.

### Durable run attribution

`invocation.json` gains an additive `listener_id: Option<String>` alongside the
existing `schedule_id`. Renaming both to `invoker_id` would be cleaner in the
abstract and would invalidate every run directory already on disk, so this stays
additive. The monitor's newest-per-schedule collapsing generalizes to key on
`schedule_id ?? listener_id`, without which a busy file listener would flood the
Automations monitor.

### Failure and degraded operation

| Failure | Behavior |
| --- | --- |
| Watch path missing or unreadable | listener stays configured, `armed: false` + `arm_error`, retried each reconcile |
| Webhook port unavailable | webhook listeners report `armed: false` with the bind error; file and poll unaffected |
| Poll request fails | record error, exponential backoff, do not fire, do not auto-disable |
| Blueprint invalid at fire time | durable failed run artifact, same as `record_schedule_launch_failure` |
| Listener deleted mid-run | the run continues — it is already an independent durable entity — and the completion write-back finds no listener and no-ops, matching `schedule::mark_run_status` |
| App not running | **file and webhook events during downtime are lost; poll recovers** because its fingerprint is durable |

That last asymmetry is a real property of the design, not an oversight: `notify`
has no journal and an unbound port cannot receive. It goes in the user docs
rather than being papered over with a scan-on-startup that would fire a
thundering herd after every restart.

### Observability

A listener that silently does nothing is indistinguishable from a listener that
is working and has nothing to report, so every refusal is recorded rather than
merely logged. `runtime.last_rejection` carries the reason and timestamp of the
most recent refused delivery — bad signature, unknown path, oversized body,
unparseable payload — which is what makes "my webhook isn't firing" a
debuggable question instead of a guess. It is deliberately last-only rather
than a ring buffer, because the config file is not a log.

Alongside it: `armed` + `arm_error` answer "is this watching at all",
`consecutive_failures` covers a flaky poll endpoint, `fire_count` and
`last_fire_epoch_ms` show liveness, and runs carry `listener_id` so the monitor
can attribute them. `wardian automation listener list` surfaces armed state and
last fire; `listeners-updated` drives the UI.

### Where the webhook server lives

A **separate** axum server from the remote gateway, with its own enable flag and
port. The gateway is opt-in, loopback-bound, and gated on P-256 device pairing —
an external webhook sender can perform none of that handshake, and a webhook
receiver that only worked when remote access was enabled would be a coupling
users cannot reason about. Different trust domain, different server.

v1 binds loopback only, matching the gateway's posture; external reach is the
user's tunnel (`cloudflared`, `ngrok`, `tailscale funnel`), which the docs show.

## Consequences

- **Positive**: one family absorbs three trigger types; a fourth is an enum
  variant rather than a seventh subsystem.
- **Positive**: one shared launch path serves all three new variants, so the
  duplication stops at two copies (`schedule.rs` and `session_close.rs`) instead
  of growing to five.
- **Positive**: poll is the variant that answers "tell me when they release a
  new version", which inbound webhooks structurally cannot do for a repository
  the user does not administer.
- **Positive**: config stays inspectable on disk (Markdown-as-Truth) because
  secrets moved out of it.
- **Negative**: an inbound HTTP server is new attack surface. Mitigated by
  loopback-only binding, auth-before-parse, and body caps — but it is new.
- **Negative**: listener-local overlap policy will need reconciling with #1008's
  general contract. The vocabulary is chosen to be absorbed, not to compete, but
  it is still a second place that knows about overlap.
- **Negative**: file and webhook events are lost while the app is down. Poll is
  the only variant that recovers, which makes the three variants behave
  differently under restart.
- **Negative**: `invocation.json` now carries two attribution fields where one
  generalized field would be cleaner, a debt paid for on-disk compatibility.
- **Negative**: `schedule.rs` and `session_close.rs` are **not** ported onto the
  shared launch path in this change. Only the deterministic-run claim is
  extracted and shared. `session_close.rs` carries a memory-principal authority
  boundary the listener path does not, and folding an authority boundary into a
  shared path inside an already-large change is how authority bugs ship;
  porting both belongs in its own reviewable change.

## Load-bearing assumption

That listener config edits are rare relative to listener *fires*. The supervisor
re-reads and reconciles the whole config on change, which is the right cost for
an occasional edit and the wrong one for a hot path. If listener config ever
becomes machine-written at high frequency, reconcile needs to become
incremental.

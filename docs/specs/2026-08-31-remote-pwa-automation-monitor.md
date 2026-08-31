# Remote PWA Automation Monitor

- **Status:** Implemented
- **Date:** 2026-08-31
- **Owner:** Wardian Architecture
- **Scope:** Remote PWA automation monitoring and the default desktop Automations mode
- **Issue:** [#1098](https://github.com/wardian-app/Wardian/issues/1098)

## Context

The remote PWA has an Automations destination, but it is a placeholder. Its
existing `GET /remote/api/automations` compatibility endpoint returns an empty
list and its TypeScript DTO exposes only an automation id, name, and node count.
The PWA therefore cannot answer the operational questions that matter on a
phone: what needs attention, what is running, what will run soon, and how recent
runs ended.

The desktop Automations workbench already combines run summaries and schedules
into a Monitor model. That model is reached through Tauri IPC and rendered with
desktop-density controls. The PWA is a separate, paired HTTPS client and must
not call Tauri IPC or compress the desktop layout into a narrow viewport.

The main desktop Automations view also defaults to Edit even though monitoring
is the primary operational use. The desktop automation sidebar is related but
is not part of this change.

## Decision

Wardian MUST add a mobile-native automation tracker to the existing remote PWA
shell. The tracker MUST consume a bounded, authenticated, audited monitor
snapshot owned by the desktop remote gateway. It MUST share automation status,
time, grouping, and tone semantics with the desktop Monitor while owning a
separate touch-first presentation.

The mobile tracker MUST use a glance-first Overview followed by progressive
disclosure. It MUST prioritize, in order:

1. runs that need attention;
2. runs in progress;
3. upcoming schedules;
4. recent completed or failed outcomes.

The main desktop Automations workbench MUST initialize and reset to Monitor.
Opening a blueprint for editing or a run for observation MUST continue to switch
to those modes explicitly.

The desktop automation sidebar MUST NOT change under this issue.

## Authority and Component Boundaries

The Rust desktop runtime remains authoritative for automation run checkpoints,
schedules, and remote-access policy. The remote gateway owns the browser trust
boundary and translates an authenticated read into narrow internal automation
queries. The PWA owns ephemeral presentation state such as the active filter,
expanded detail, retained pages, loading state, and the last successful
snapshot.

The implementation MUST preserve these dependency directions:

```text
run checkpoints + schedules
          |
          v
Rust bounded readers --> remote gateway DTO --> remote client/store
                                                |
                                                v
                              shared monitor semantics
                                                |
                                                v
                                  mobile tracker UI
```

The remote UI MUST NOT own scheduler state, infer missing runs from Inbox
notifications, read automation files directly, or route arbitrary browser JSON
through the local CLI control dispatcher.

## Remote Monitor Contract

### Request

The gateway MUST expose:

```http
GET /remote/api/automations/monitor?active_offset=<non-negative integer>&recent_offset=<non-negative integer>&schedule_offset=<non-negative integer>
```

All offsets default to zero. Page sizes are server-owned constants. A caller
MUST NOT be able to increase the size of a single request or response by
supplying a limit. Loading more MUST advance one bounded offset and merge that
page into the client projection.

The endpoint is read-only but sensitive. It MUST enforce the same exact host,
origin, authenticated session, and request-boundary checks as other remote
reads. Every accepted or rejected request MUST retain remote audit provenance.

### Response

The normative response has this shape:

```ts
interface RemoteAutomationMonitorSnapshot {
  schema_version: 1;
  generated_at: string;
  active_runs: RemoteAutomationMonitorRun[];
  active_runs_truncated: boolean;
  active_runs_next_offset: number | null;
  recent_runs: RemoteAutomationMonitorRun[];
  recent_runs_truncated: boolean;
  recent_runs_next_offset: number | null;
  schedules: RemoteAutomationMonitorSchedule[];
  schedules_truncated: boolean;
  schedules_next_offset: number | null;
}

interface RemoteAutomationMonitorRun {
  run_id: string;
  blueprint_id: string;
  automation_name: string;
  schedule_id: string | null;
  status: "running" | "awaiting_approval" | "completed" | "failed";
  node_count: number;
  completed_node_count: number | null;
  failure: string | null;
  started_at: string | null;
  updated_at: string | null;
  completed_at: string | null;
}

interface RemoteAutomationMonitorSchedule {
  id: string;
  blueprint_id: string;
  automation_name: string;
  schedule: ScheduleDefinition;
  next_run_epoch_ms: number | null;
  is_paused: boolean;
  last_run_status: "running" | "awaiting_approval" | "completed" | "failed" | null;
  last_run_error: string | null;
  last_run_epoch_ms: number | null;
  target_labels: string[];
}
```

`generated_at` MUST be an RFC 3339 timestamp generated after the snapshot data
has been read. Every non-null run timestamp MUST also be RFC 3339.
Schedule times remain epoch milliseconds to match the authoritative scheduler
DTO.

`active_runs` contains running and awaiting-approval runs and is ordered by the
most recent authoritative update. `recent_runs` excludes active runs and is
ordered newest first. `schedules` is ordered by operational relevance: active
upcoming schedules by next execution, followed by paused schedules ordered by
last-run time descending, then automation name and schedule id ascending.

`completed_node_count` MAY be null when progress cannot be derived reliably.
The PWA MUST omit progress rather than manufacture it. `failure` and
`last_run_error` MUST be stable remote-safe summaries, never persisted provider
or filesystem error text, and MUST be bounded and safe for rendering. The response MUST
NOT include node outputs, event logs, automation input payloads, raw bindings,
provider transcripts, PTY content, local filesystem paths, or reusable
credentials.

`target_labels` is a sanitized display projection. It MAY contain agent names,
roles, or temporary-provider labels, but MUST NOT contain workspace paths. An
empty list means no safe target label is available; it does not mean the
automation has no runtime assignment.

All three collections MUST have fixed server-side page caps and independent
offsets. Truncation MUST be explicit. Each `next_offset` MUST be null when no
later page is available. An offset beyond the end MUST return an empty page
with a null next offset rather than an error.

Offset pagination is intentionally best-effort over a changing local runtime.
The client MUST merge pages by stable collection identity (`run_id` or schedule
`id`) and ignore duplicates. A background or explicit refresh MUST replace the
retained first page and reconcile previously retained items by identity. A run
that changes collections between page requests MAY be temporarily omitted or
duplicated before de-duplication; the next first-page refresh is the recovery
boundary. The API does not promise cursor-stable historical traversal.

### Consistency and Partial Failure

The snapshot is a read projection, not a transaction across files. A schedule
may advance while run checkpoints are being read. Each returned item MUST be
internally valid, and `generated_at` communicates freshness, but the API does
not promise a globally atomic scheduler snapshot.

One unreadable run directory MUST be skipped using the existing tolerant run
listing behavior. A top-level inability to read run or schedule state MUST
return a typed service-unavailable error; the gateway MUST NOT turn missing
authoritative data into a successful empty monitor.

The PWA MUST keep its last successful snapshot visible when a background
refresh fails and mark it stale. The remote shell and agent watchlist MUST
remain usable when the optional automation monitor endpoint is missing or
temporarily unavailable.

An HTTP 404 means the paired desktop build does not support this capability and
MUST render an `Update the desktop app to use automation monitoring.` state
without retry polling. A typed service-unavailable response means the capability
exists but authoritative state could not be read and MUST render the retryable
first-load or stale-refresh state described below.

## Shared Monitor Semantics

Desktop and remote presentation MUST derive their interpretation from one pure
TypeScript semantic layer over narrow run and schedule inputs. That layer owns:

- attention, running, scheduled, and history grouping;
- run and schedule status labels;
- semantic tones;
- chronological ordering;
- calendar-aware time and duration formatting.

The normative Attention predicate is:

- every run whose status is `awaiting_approval`;
- the newest non-active run for an automation when that run failed and has not
  been superseded by a newer completed or active run;
- a schedule whose `last_run_status` is `failed` when no newer run record for
  that schedule is present in the retained projection.

A paused schedule is not attention by itself. A running run is not attention
unless a separate retained run or schedule satisfies the failure rule. The
shared semantic layer MUST apply this predicate for both desktop and remote
counts and grouping.

Desktop cards and mobile cards MAY differ structurally. Sharing React desktop
cards with the PWA is explicitly not required and SHOULD be avoided when it
introduces desktop controls, density, or Tauri dependencies into the remote
bundle.

## Mobile Information Architecture

The Automations destination remains inside the existing five-item bottom
navigation and safe-area shell. The tracker uses one vertical scroll region and
a sticky header.

The header MUST show `Automations`, a last-successful-refresh label, and a
minimum 44 by 44 CSS-pixel refresh target. Refresh on window focus, page show,
and visibility resume remains automatic. An explicit refresh MUST not discard
visible data while the request is in flight.

Below the header, the UI MUST expose four filters:

- **Overview** (default)
- **Attention**
- **Soon**
- **History**

The active filter is ephemeral and MAY be retained for the current PWA session;
it does not become desktop or backend state. Overview contains the prioritized
sections below. Empty sections SHOULD be omitted from Overview after the
summary shortcuts, while a directly selected empty filter MUST show a
section-specific empty state.

### Summary Shortcuts

Overview starts with three large, tappable shortcuts for attention, running,
and upcoming schedules. Each shortcut changes the active filter or scrolls to
the corresponding section. They are navigation, not a compressed five-column
desktop statistics panel.

### Activity Cards

Every card MUST present one dominant fact:

- attention: the required action or failure;
- running: status and trustworthy progress, when available;
- upcoming: relative time first and exact local time second;
- history: completed or failed outcome and when it ran.

The complete card is the primary touch target. Cards SHOULD disclose secondary
metadata in an accessible mobile detail sheet or equivalent single-detail
surface. The detail surface MAY show exact timestamps, duration, recurrence,
target labels, and bounded failure text. It MUST NOT expose raw run output or a
desktop DAG.

The initial tracker is read-only. Existing remote run and stop transport methods
remain available to their existing callers but MUST NOT gain new controls in
this tracker. A later action design requires separate scope and confirmation
rules. No required interaction may depend on a swipe, hover, long press, or
icon-only affordance.

### Visual Language

The tracker MUST use Wardian theme variables and themed classes. Hard-coded
Tailwind palette colors are forbidden. Cards use a neutral surface with a
narrow semantic accent, icon, and text label:

- cyan: running;
- amber: awaiting action or paused;
- red: failed;
- emerald: completed;
- gray: inactive or unavailable.

Color MUST NOT be the only status signal. Primary body text SHOULD be 14 to 16
CSS pixels and metadata MUST NOT be smaller than 12 CSS pixels. Interactive
targets MUST be at least 44 by 44 CSS pixels with at least 12 CSS pixels between
adjacent consequential actions.

Pressed, keyboard-focus, disabled, and loading states MUST be visible. Motion
MUST respect `prefers-reduced-motion`. The design MUST NOT depend on hover.

## Loading, Empty, Stale, and Error States

The first load SHOULD use fixed-height skeleton cards so the header and bottom
navigation remain stable. Refreshing an existing snapshot keeps cards visible.

The following empty messages are distinct:

- `Nothing needs attention.`
- `No automations are running.`
- `No schedules are coming up.`
- `No recent automation outcomes.`

A failed refresh with cached data shows a compact stale banner, the last
successful timestamp, and a Retry action. A failed first load shows an inline
error panel inside the Automations destination; it MUST NOT replace the remote
pairing or connectivity state machine.

Loading additional active runs, schedules, or history uses a full-width labeled
button with a minimum 44-pixel height in the relevant filtered view. The UI MUST
announce appended results through a polite live region and preserve scroll
position. Concurrent requests for the same collection and offset MUST coalesce;
the UI MUST NOT emit a duplicate request or show an avoidable error.

## Responsive and Accessibility Behavior

The primary layout is a single column from 320 CSS pixels upward. Content
SHOULD be centered and capped near 720 CSS pixels on wider mobile and tablet
viewports. A wider Overview MAY place Running and Soon sections in two columns,
but History remains one chronological stream.

The tracker MUST account for the bottom navigation safe area. A detail sheet
MUST trap focus, restore focus to its originating card, close through an
explicit button and browser Back, and remain usable with the software keyboard.
Status changes and stale-state transitions MUST have screen-reader text.

## Desktop Default Mode

`useAutomationsView` MUST initialize with `mode: "monitor"` and its `reset()`
operation MUST restore Monitor. Existing explicit transitions to Edit and
Observe remain unchanged. Tests that assumed Edit as the implicit initial mode
must set Edit explicitly when authoring behavior is under test.

## Security and Privacy

The existing paired-device model remains unchanged. This feature introduces no
new credential, cookie, WebSocket, public relay, or offline mutation mechanism.

Monitor reads MUST be audited as automation reads. Error responses MUST use
stable codes and MUST NOT reveal local paths or raw filesystem errors. The PWA
service worker MUST NOT cache monitor API responses. Browser storage MAY retain
only non-sensitive presentation preferences, not monitor payloads.

## Non-Goals

- A responsive copy of the desktop Automations workbench
- The visual builder, run DAG, node outputs, or event timeline in the PWA
- Desktop automation sidebar changes
- Public or hosted automation monitoring
- Offline run, stop, pause, resume, or retry queues
- Push notifications or a new automation WebSocket stream
- Redesigning scheduler persistence or run-checkpoint storage

## Alternatives Considered

### Reuse the desktop monitor React tree

Rejected. The desktop tree depends on Tauri stores, desktop-density cards, and
multi-control layouts. Making it responsive would couple the remote bundle to
the wrong authority and interaction model.

### Expose separate raw run and schedule endpoints

Rejected for the initial surface. It would require the phone to coordinate
multiple refreshes and would expose more backend structure than the tracker
needs. A narrow monitor snapshot is easier to bound, audit, and evolve.

### Return a fully grouped, display-ready Rust view model

Rejected. Rust owns authoritative facts and the trust boundary; TypeScript
already owns desktop monitor interpretation. Keeping grouping and labels in a
shared pure TypeScript layer prevents semantic drift without making React
components shared.

### Keep Edit as the desktop default

Rejected by product priority. Authoring remains one explicit mode, while
Monitor is the primary landing surface for operational awareness.

## Verification and Acceptance Evidence

The lowest meaningful test layer MUST be used:

- Rust unit tests: pagination bounds, ordering, sanitization, truncation,
  unreadable-item tolerance, top-level failures, route authentication, audit
  events, and response error codes.
- Frontend unit tests: shared grouping, filter counts, mobile card content,
  touch-target classes or computed dimensions, stale refresh, pagination
  merging, duplicate-request suppression, and Monitor default/reset behavior.
- Browser E2E: the remote mobile route renders attention, running, upcoming,
  recent, empty, loading, stale, and truncated-history states; filters and
  detail disclosure work at a phone viewport; no mutation is queued offline.
- Native runtime E2E: a persisted schedule and real run checkpoint are visible
  through the authenticated remote gateway. Browser-only mocks cannot prove
  this boundary.
- Visual evidence: at least one feature-specific phone screenshot showing the
  Overview hierarchy is uploaded and embedded in the PR description.

The implementation is complete only when `npm run verify:ci` passes, the PR has
no merge conflict, required CI checks are green, and Wardian-Reviewer records a
verdict with zero blocking findings.

## Consequences

The PWA gains a useful operational surface without weakening Wardian's
local-first authority or copying desktop chrome. Shared semantics reduce status
drift while independent presentation preserves touch usability.

The gateway gains another sensitive read contract and must maintain pagination,
sanitization, and audit tests. The snapshot is intentionally not globally
atomic, so the UI must communicate freshness and tolerate a schedule moving
between refreshes. A future live automation stream can replace polling without
changing the mobile information hierarchy, but it is not required here.

# Runtime CPU and Memory Profile

## Status

Baseline and source-level attribution were captured from `origin/main` at
`f07981e8`. The optimized production build was measured against the same loaded
roster, and the reusable profiling tools remain checked in for future work.

## Problem

Wardian can supervise dozens of long-lived provider sessions. Operating-system
process totals do not explain whether resource consumption belongs to Wardian,
its WebView UI, or the provider workloads it owns. Optimization therefore needs
a component-level baseline that keeps those categories separate and can be
repeated against the same loaded-roster workload.

## Measurement contract

The representative workload is a warm Windows desktop runtime with 64 known
agents, 38 live provider sessions, and 36 ConPTY hosts. The primary metric is
average CPU cores consumed by `Wardian.exe` over a 30-second window. Supporting
metrics are backend private memory, WebView private memory, supervised-runtime
private memory, process counts, thread counts, and Windows process I/O counters.

Run the checked-in Windows profiler from PowerShell. If more than one Wardian
process is running, select the intended process explicitly:

```powershell
./scripts/profile-wardian-runtime.ps1 -DurationSeconds 30 -WardianProcessId <pid> -OutputPath <profile.json>
```

The script records no command lines, environment values, agent names, workspace
paths, or executable paths. A before/after comparison is valid only when the
live-agent and ConPTY counts are comparable. The first optimization target was
at least a 50% reduction in backend CPU cores without a material backend-memory
regression.

Correctness remains a hard floor: provider status and readiness must remain
current, PTY output and input must not be lost, terminal ownership must remain
ordered, telemetry must eventually converge, and idle mailbox delivery must not
be delayed beyond its existing contract.

## Baseline

The loaded runtime showed the following stable component split:

| Component | CPU | Working set | Private memory | Interpretation |
| --- | ---: | ---: | ---: | --- |
| Wardian backend | 2.626 cores (8.21% of 32 logical CPUs); 2.79-core lifetime average | 289 MiB | 325 MiB | Primary application CPU consumer |
| WebView2 UI | 0.429 core | 783 MiB | 684 MiB | Secondary CPU consumer; material memory consumer |
| Supervised providers and helpers | 0.411 core | 6.9 GiB | 8.8 GiB | Dominates total memory, but belongs to agent workloads rather than Wardian itself |

The frozen 30-second run lasted 30.812 seconds and sampled 134 descendants. The
full process tree used about 8.0 GiB of working set and 9.8 GiB of private
memory. The Wardian backend plus WebView accounted for about 1.0 GiB private;
providers and their helpers accounted for the remaining 8.8 GiB.

Backend private memory was flat within each short sample. A long-running process
used roughly 100 MiB more private memory than a separate warm process, which is
enough to justify a later long-duration heap profile but is not evidence of an
unbounded leak.

## CPU attribution

The frozen run recorded 229.5 MiB/s and 58,660 read operations per second. Other
short samples ranged from 279-348 MiB/s and 70,000-92,000 operations per second,
with an average operation close to 4 KiB. Physical
disk reads averaged only 0.14 MiB/s during a concurrent sample. These are
therefore in-memory or IPC reads, not storage traffic.

The one-second trace was continuously busy rather than showing a five-second
spike. Wardian has one blocking PTY reader per live terminal and provider-log
watchers polling every 250 ms, but process-wide counters cannot distinguish
those callers. A source-level profiler now records the exact boundaries below;
the leading source must be selected from its measured shares rather than from
thread counts alone.

| Metric | Boundary | Unit |
| --- | --- | --- |
| `pty_read` | Bytes returned by provider ConPTY reads | bytes |
| `terminal_broker_output` | Terminal filter, VT parser, and event publication | input bytes |
| `pty_postprocess` | Provider/status parsing after broker acceptance | input bytes |
| `*_watcher_poll` | One provider-log watcher pass, excluding its sleep | polls |
| `provider_log_read` | Incremental JSONL records consumed by watchers | bytes |
| `antigravity_latest_step` | SQLite user-step receipt query | rows found |
| `antigravity_message_scan` | SQLite conversation-message projection, full on initial position and bounded afterward | projected messages |
| `telemetry_ingest_discover` | Provider source-topology discovery | sources |
| `telemetry_ingest_pass` | Incremental persisted telemetry ingest | sources |
| `telemetry_fleet_query` | Dashboard fleet-summary query | queries |
| `telemetry_matrix_query` | Analytics matrix query | queries |
| `inbox_approval_scan` | Approval-source Inbox projection | scans |
| `inbox_terminal_scan` | Automation-run Inbox projection | scans |
| `app_metrics` | App/process-tree aggregation | passes |
| `metrics_tick` | End-to-end metrics heartbeat | agents |

Synchronous low-frequency boundaries report both wall and current-thread CPU
time. Async boundaries report wall time because a future can resume on a
different worker thread. The high-frequency PTY boundaries use wall time to
avoid adding two thread-accounting system calls to every terminal chunk.
Nested timings are not additive: for example, Antigravity message projection is
part of the corresponding watcher pass.

Enable source-level accounting before starting Wardian:

```powershell
$env:WARDIAN_RUNTIME_PROFILE = '1'
$env:WARDIAN_RUNTIME_PROFILE_INTERVAL_SECONDS = '10'
& '<workspace-path>\target\release\Wardian.exe'
```

The production binary must be built with `tauri/custom-protocol`; a plain Cargo
release build retains the development `localhost` URL and is not a valid UI
profile. See [Runtime Resource Profiling](../developer/runtime-resource-profiling.md).

The profiler appends one aggregate JSON object per interval to
`<wardian-home>/wardian_debug.log`. It records no agent IDs, names, paths,
terminal text, provider payloads, or other user content. With profiling unset,
hot paths perform one cached Boolean check and do not read clocks or update
counters.

## Matched source-level attribution

A controlled restart of the diagnostic instrumented release produced a 60.8-second
process sample paired with six ten-second source-accounting intervals. The
restored runtime had four Antigravity processes and 61 known agents. Wardian's
backend averaged 0.592 CPU core, held 117 MiB private memory, and was flat over
the sample. WebView2 held 186 MiB private, while supervised providers and
helpers held 6.45 GiB private.

| Boundary | CPU cores | Share of backend | Calls in 60 s | Interpretation |
| --- | ---: | ---: | ---: | --- |
| `antigravity_watcher_poll` | 0.478 | 80.7% | 32 | Dominant hot path |
| `telemetry_scan` | 0.045 | 7.5% | 2 | Second-largest measured path |
| `antigravity_message_scan` | 0.010 | nested | 32 | Full-history payload projection |
| `codex_watcher_poll` | 0.002 | 0.4% | 5,749 | Poll frequency is high, work is cheap when unchanged |
| `process_refresh` | 0.002 | nested | 2 | Not the telemetry scan's main cost |

The dominant call is identity discovery, not terminal processing. The outer
Antigravity watcher used 28.66 CPU-seconds; its measured latest-step and
message-scan children used only 0.72 CPU-second. The remaining 27.94
CPU-seconds, or 0.466 core, surround the unconditional call to
`fresh_database_conversation_for_workspace` and its fallback. This is 97.5% of
the outer watcher's CPU and 78.7% of total backend CPU.

The reason is a control-flow error. Each restored agent already has a durable
`resume_session`, but the watcher calculates `discovered` before
`antigravity_watcher_conversation` selects that existing identity. Restored
agents also have an empty launch baseline. Every nominal 250 ms poll therefore
enumerates all 165 Antigravity conversation databases and opens every candidate
to select and lowercase its complete trajectory-metadata blob. The local store
contains 314 MiB across those databases. The matched process sample recorded
170 MiB/s and 43,017 reads/s while PTY and incremental provider-log reads were
nearly absent; divided by the 0.53 completed discovery polls/s, that is about
320 MiB of process reads per poll, closely matching the entire 314 MiB store.
Each poll consequently took 6.68 seconds wall-clock on average instead of 250
ms.

The first optimization should short-circuit discovery whenever the config has
a non-empty `resume_session`. For fresh sessions, discovery should run only
until identity is captured and should retain the launch baseline. This removes
the measured 0.466-core path without changing identity authority. A second
Antigravity optimization should replace the 250 ms full-history message decode
with an incremental cursor query, ideally reusing one read-only connection and
checking SQLite's data version before fetching new steps. Otherwise the current
0.010-core message scan will rise toward roughly 0.3 core after the blocking
identity scan is removed and all four watchers can actually reach their nominal
cadence.

The metrics supervisor is the second CPU target and a separate latency issue.
During the matched sample, two all-agent scans used 2.67 CPU-seconds and 4.09
seconds wall-clock. Only 0.13 CPU-second belonged to `sysinfo` process refresh.
The complete ticks spent 48.59 seconds wall-clock, including 24.83 seconds in
serial status application and 28.39 seconds across 77 mailbox-drain attempts.
Those async phases consumed little directly measured CPU, but they delay status
and mailbox convergence. Optimization should separate resource sampling from
provider-log/status reconciliation and trigger mailbox delivery from readiness
transitions rather than serially sweeping every ready agent on the metrics
heartbeat.

## Implemented optimization

The Antigravity watcher now defers conversation discovery until it proves that
the agent has no persisted provider identity. Restored agents therefore return
their existing identity without enumerating or opening unrelated conversation
databases.

Once an identity is known, the watcher compares the main database and write-ahead
log size and modification time before querying. An unchanged source performs no
SQLite projection. A changed source reads an inclusive 16-step overlap instead
of decoding the full transcript; the overlap preserves updates to the current
planner step while bounding retained deduplication state. The initial restored
read remains complete so existing transcript history is positioned rather than
re-emitted.

Telemetry-triggered mailbox delivery now runs only when a provider generation
transitions into ready state. Repeated telemetry observations of an already-ready
generation no longer sweep the same mailbox. Manager-owned readiness events
retain their immediate delivery path.

Provider processes remain a separate resource category. Wardian already exposes
explicit pause and off states that terminate their runtime processes. This
change does not add automatic provider termination, because an idle-time or
memory policy would alter agent lifecycle semantics and requires a separately
specified user control. Provider memory still remains an optimization priority;
it is not counted as evidence that Wardian's own footprint is acceptable.

The automation Inbox projection now caches parsed run summaries by run state,
coalesces concurrent catalog scans, filters before pagination, and rejects stale
background results after a successful Inbox mutation. Terminal run summaries
are immutable, so completed and failed runs no longer incur filesystem metadata
checks on every refresh. The original uncached path scanned 2,338 runs and 7,115
files (188 MiB) and consumed 29.56 CPU-seconds in one request.

The shared Codex index now retains source and target metadata and skips an
unchanged projection. Source growth remains append-preserving; a missing,
shrunk, or replaced target still forces reconstruction. This keeps the existing
recovery behavior while removing a recurring 0.75 to 1.17 CPU-second pass.

Workbench Dashboard and Analytics surfaces retain their rendered snapshot when
hidden, but suspend their telemetry queries until visible. This preserves tab
state without making every hidden surface poll the backend. The terminal
renderer budget was not reduced: the measured workload mounted only one xterm,
so its existing budget was not the source of the memory footprint.

Finally, background telemetry ingest separates source topology from source
contents. Projected provider homes that resolve to the shared Codex or Claude
root reuse the machine catalog instead of recursively walking the same tree per
agent. The resolved topology is retained between one-minute delta passes,
invalidated immediately by agent, provider, session, or workspace changes, and
refreshed after five minutes to discover unobserved headless sessions. Explicit
telemetry refresh still performs a complete discovery.

## Optimization verification

The final matched 60.8-second production sample contained 61 known agents and a
comparable live provider tree. It produced this component split:

| Component | Origin-main baseline | Optimized | Change |
| --- | ---: | ---: | ---: |
| Wardian backend CPU | 2.626 cores | 0.194 core | -92.6% |
| Backend process reads | 229.5 MiB/s | 14.9 MiB/s | -93.5% |
| Backend private memory | 325 MiB | 127 MiB | -60.9% |
| WebView2 private memory | 684 MiB | 432 MiB | -36.8% |
| Supervised-runtime private memory | 8.8 GiB | 6.2 GiB | workload-dependent |

The most conservative CPU comparison uses the first valid production build
after the Antigravity correction (2.569 cores and 95.3 MiB/s reads): the final
build still reduced backend CPU by 92.4% and process reads by 84.4%. Backend
private memory varied with process age and cache state; the measured final value
was lower than origin main, but the short samples do not establish a heap-growth
rate. A preceding matched 60-second sample measured 0.176 backend core; the
table reports the exact final binary's later quiet sample.

In the final diagnostic source-level sample, hidden Dashboard and Analytics queries were
absent. The largest recurring boundaries were approval projection (1.41 CPU-s
per minute), telemetry scan (1.34 CPU-s), agent reconciliation (0.78 CPU-s),
provider-log reconciliation (0.52 CPU-s), terminal Inbox projection (0.52
CPU-s), and Codex indexing (0.34 CPU-s). The then-once-per-minute topology
discovery used 2.81 CPU-s to resolve 304 sources; the final cache and shared-root
change removes that work from ordinary one-minute passes while preserving its
bounded discovery contract. In the final production build, the first populated
discovery resolved 305 sources in 284 ms wall and 266 ms CPU (about 90% less CPU
than the prior pass), and three following passes reused that topology with no
discovery call.

WebView inspection attributed the largest process to the renderer, followed by
the GPU process. The live page contained roughly 13,500 DOM nodes, 8,000 to
10,000 event listeners, one xterm with three canvases, and one visible graph.
JavaScript live heap was only 27 to 42 MiB. A diagnostic forced collection
reduced renderer private memory from roughly 402 MiB to 160 MiB before it later
regrew, indicating substantial reclaimable allocation churn rather than a
single comparably large live JavaScript leak. Hidden surfaces remain a future
memory target, but any unmounting policy needs explicit state-restoration UX.

Kernel CPU stack capture was not available under the current Windows tracing
policy. A debugger-based alternative was rejected after it terminated one
profiled Wardian process; it must not be used again for this workload.

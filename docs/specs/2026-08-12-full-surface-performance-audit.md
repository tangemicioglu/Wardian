# Full-Surface Performance Audit

Date: 2026-08-12

## Purpose

Wardian must remain responsive at the habitat size used in normal operation,
not only with the small fixtures used by functional browser tests. This audit
defines one repeatable production-renderer workload for every registered
workbench surface and the shared shell interactions that affect them.

The live habitat was inspected read-only to establish scale. It contained 54
agents: 33 idle, 20 off, and one processing. Mutating measurements use an
isolated deterministic fixture with the same distribution so benchmark runs
cannot rename, select, send to, or otherwise alter live agents.

## Audited surface matrix

The fixture restores 20 tabs in four groups and contains every registered
surface type:

| Surface | Measured interaction |
|---|---|
| New Tab | Open and close the searchable surface launcher |
| Agents | Filter agents and switch Auto/Grid presentation |
| Dashboard | Select an agent card |
| Inbox | Open and close event filters with 24 seeded items |
| Graph | Activate the 34-agent topology and rerun layout |
| Garden | Activate the habitat and open/reset its context menu |
| Library | Search the library |
| Workflows | Open and close the node library |
| Agent Session | Switch owner/mirror tabs and commit terminal bursts |
| Browser | Submit an address through an attached mock browser lease |
| Files | Render Markdown and switch to the source editor |

Shared coverage includes five cold restores, first activation and steady tab
switching by surface type, 20 group-focus changes, all six left-sidebar panes,
right-roster filtering, Settings open/close and all eleven Settings categories,
ten full-roster telemetry updates, six Agents resize cycles, four Graph/Garden
release-resume cycles, React commit duration, terminal stream gaps, and peak
xterm/WebGL allocations.

## Measurement contract

- Build and serve Wardian through Vite's production build and preview APIs.
- Use `react-dom/profiling`; do not substitute a development renderer.
- Start interaction timing at user input and stop after two animation frames
  and the interaction-specific ready condition.
- Report first activation separately from steady tab switching. A benchmark
  must not preload hidden GPU surfaces merely to improve its tab-switch score.
- Fail when an expected surface, interaction target, React commit, terminal
  acknowledgement, or browser error is missing.
- Keep the fixture and all benchmark writes inside an explicit isolated
  `WARDIAN_HOME`. The default result is
  `WARDIAN_HOME/workbench-performance-baseline.json`; updating the checked-in
  baseline is a separate, reviewed copy from that isolated result.
- Compare bundle size with the frozen gzip size measured on the origin/main
  source used to start the audit.

The 54-agent gates deliberately differ from the former 20-agent navigation
profile. They are regression limits, not claims that cold and warm operations
have identical costs:

| Measure | Limit |
|---|---:|
| Restore p95 | 1,500 ms |
| First surface activation p95 | 500 ms |
| Steady tab switch p95 | 250 ms |
| Group focus p95 | 175 ms |
| Terminal output commit p95 | 50 ms |
| Full-roster telemetry p95 | 100 ms |
| Agents resize settle p95 | 300 ms |
| Graph/Garden resume p95 | 500 ms |
| Cross-surface interaction p95 | 300 ms |
| Maximum React commit | 80 ms |
| Bundle gzip delta | 250 KiB |
| Peak xterm renderers | 24 |
| Peak WebGL contexts | 12 |
| Terminal stream gaps | 0 |

## Lifecycle decisions

Parent updates are deferred while Agents, Graph, or Garden is hidden. The
component remains mounted and keeps local state; it receives current parent
props on reveal. The visible-to-hidden transition still renders once so a
heavy renderer can observe visibility and release after its grace period.

A Graph or Garden surface restored hidden starts with its expensive renderer
released. The renderer mounts on first reveal. A renderer that was already
visible retains the existing grace period so ordinary short tab switches do
not churn WebGL resources.

Dense Dashboard, Agents, and roster rows use CSS content visibility with an
intrinsic block-size estimate. Agents chat bodies additionally use the existing
viewport residency observer: chat mode and draft state remain parent-owned,
while an offscreen body is suspended until its card approaches the viewport.

## Running the audit

POSIX shell:

```sh
export WARDIAN_HOME="<absolute-workspace-path>/.tmp/workbench-performance/manual"
npm run perf:workbench
```

PowerShell:

```powershell
$env:WARDIAN_HOME = "<absolute-workspace-path>\.tmp\workbench-performance\manual"
npm run perf:workbench
```

Use `--audit --output <absolute-path-inside-WARDIAN_HOME>` while diagnosing a
failing candidate. That mode records all observations without converting a
failed gate into an early exit. `--reuse-build` and `--reuse-bundle` are local
iteration aids; the committed baseline must come from a complete build. Use
`--screenshot <absolute-path-inside-WARDIAN_HOME>` to capture the final
54-agent Agents view for PR evidence without changing the benchmark fixture.

## Acceptance

- The complete production benchmark passes every gate with the 54-agent
  fixture.
- Unit coverage proves hidden prop deferral, reveal catch-up, initially hidden
  heavy-renderer release, and offscreen chat suspension semantics.
- Frontend lint, unit tests, and production build pass.
- Backend checks remain green because the benchmark and fixes do not change the
  Rust runtime or persistence contracts.

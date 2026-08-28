# Debt gates

## Context

An audit of the thirty pull requests merged to `main` between 24 and 28 August
2026 (#949 through #1015) found a consistent shape. Twenty-two were labelled
`fix`. Together they added 26,584 lines and removed 3,371, a ratio near eight
to one. Three separate defects each took two or three merged PRs to settle:
#956 → #983 → #990 widened one regex in the chat rendering layer three times,
#988 → #991 chased desktop freezes with caches rather than a rule, and
#962 → #964 fixed the in-app browser twice from user reports.

The important part is what the audit did not find. There are no `TODO`
markers, no `@ts-ignore`, effectively no `any`, and a real test culture. The
pre-commit checklist in `AGENTS.md` was followed closely on every one of the
thirty.

That rules out the obvious response. Writing more rules cannot help, because
the existing rules were obeyed. Every finding instead traces to a single
defect: **a checklist item that is satisfiable without the property it names
being true.**

- `npm run lint` passed while no linter existed; it was aliased to
  `tsc --noEmit`.
- "I added tests" was true while no job ran them. 44 Rust browser tests sat
  behind `#[ignore]` with no CI step passing `--ignored`. CI named 4 of 46
  native E2E files by hand. 19 browser E2E cases were `test.skip`.
- `cargo fmt` was in the contributor checklist but not in CI; 481 hunks across
  76 files had drifted.
- The screenshot gate matched the PR body against an image-URL regex, which
  cannot tell whether the image shows the change.

An agent optimising against a gate will satisfy the gate. That is correct
behaviour and precisely the problem.

## Decision

Convert each expectation into something a machine can fail the build over, and
pay off the debt that the new gates would otherwise start red on.

### Pay off what was already located

- Delete `workflow_list_runs_from_root`, the `#[cfg(test)]` fork of workflow
  run listing created when #981 added pagination. Two of three run-listing
  tests exercised a copy that never shipped, and the copies had diverged.
- Collapse the `Page | T[]` union at 28 call sites across 9 files. No command
  returns the array shape; the unit-test fixtures did, so production carried a
  permanent branch to keep stale fixtures working. Correct the fixtures.
- Delete 8 unreachable modules and 31 unreferenced exports. Keep and tag
  `@ipcContract` the 23 types that mirror live Rust request and response
  structs: absence of a TypeScript consumer is not evidence a contract type is
  dead.
- Run `cargo fmt --all` once, alone, so the gate starts from zero.

### Make each gate check what it claims

- `npm run lint` means ESLint. The old alias becomes `npm run typecheck`.
- The browser-session suite runs in the existing Windows backend job. It needs
  a Chromium-based browser; that runner ships Edge and `engine_candidates()`
  looks for Edge first, so it needed one step and no code change.
- Every `e2e-native/tests/*.test.mjs` declares `// @tier ci|nightly|manual`.
  CI runs the `ci` tier, a nightly workflow runs `nightly`, and `manual` names
  a real-provider gate. Selection by tier replaces the hand-written file list.
- `cargo fmt --all -- --check` runs in CI.
- `check:test-reachability` fails on a native test with no tier and on a
  `test.skip` with no tag. Existing skips are listed explicitly with the reason
  each is still present; that list may only shrink.
- `check:deadcode` runs knip over the same include set that was verified clean.

### Ratchet what cannot be fixed at once

`budgets.json` freezes seven metric groups at their measured values: per-file
line counts for the modules that only grew, clippy suppression counts,
`#[cfg(test)]` seams, ignored Rust tests, skipped E2E cases, and ESLint
warnings. CI fails when a number rises. A change that improves one lowers it in
the same commit.

Per-file line budgets rather than a single total: one file shrinking must not
buy room for another to grow.

This generalises `verify-workbench-cutover.mjs`, which was already a rule
engine with ids, path scoping, and allowlisted matches carrying stated reasons.
The machinery existed; it was built for one migration.

### Name the caps as one policy

The eleven collection bounds introduced by #981 lived as magic numbers in nine
files. They move to `crates/wardian-core/src/limits.rs`, beside `paths.rs`.

## Invariants

1. A test that no job executes fails a gate rather than counting as coverage.
2. A metric in `budgets.json` may fall but never rise without a deliberate,
   reviewed edit to the budget.
3. `npm run <name>` does what `<name>` says.
4. A rule lands as `error` only where the codebase is already clean, so a
   violation always means a new one rather than an old one.
5. A contract type kept without a consumer carries `@ipcContract` and names the
   Rust struct it mirrors.

## Verification

- `npm run typecheck`, `npm run lint` (0 errors), `npm run test`,
  `npm run build`.
- `npm run check:test-reachability`, `check:deadcode`, `check:budgets`,
  `check:workbench-cutover` — all pass.
- `cargo clippy --workspace -- -D warnings` and `cargo fmt --all -- --check`
  pass.
- `cargo test --workspace -- --test-threads=1`: 1,695 passed, 45 ignored, and
  one failure. The failure is `snapshots_stay_within_their_measured_budgets`,
  the wall-clock p95 gate, on a run that shared the machine with a concurrent
  vitest suite. It passes alone in 7.2s. PR #991 disclosed the same test
  failing for the same reason; a timing assertion that depends on machine load
  is worth revisiting, and is not changed here.
- `cargo test --lib browser_session::tests -- --ignored --test-threads=1`:
  44 passed, 0 failed, 40s. These had never been executed by any job.

## Known-unresolved

**The frontend suite is nondeterministic, and this change does not fix it.**
`RemoteMobileApp.test.tsx` passes in isolation on every run and fails
intermittently in the full suite. Three consecutive full-suite runs at
`origin/main`, before any change here, failed 3, then 2, then 1 test. The cause
is cross-file state leakage; `useRemoteStore.ts` holds twelve module-level
mutable variables including live timers. This predates the work here and needs
its own change.

Two Phase 3 items from the remediation plan are deliberately not in scope:

- Moving provider-label classification behind the Rust normaliser and deleting
  the three frontend regexes. #990 already built the alias resolver; finishing
  the move is a behavioural change to the chat surface and belongs in its own
  PR with its own screenshot evidence.
- Making `Page<T>` a discriminated result so the 29 catch blocks that return an
  empty list on IPC failure stop compiling. This is the right fix for
  presenting an unreachable desktop as a quiet one, and it touches every list
  surface.

One thing this surfaced rather than fixed: `telemetry_dashboard` is a
registered Tauri command whose only TypeScript client was an unreachable
module. Its DTO types are retained and tagged.

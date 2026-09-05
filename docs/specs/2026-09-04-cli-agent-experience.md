# CLI agent experience audit

Tracking issue: [#1120](https://github.com/wardian-app/Wardian/issues/1120).

## Outcome and scope

Make discovery cheap, arguments predictable, and results actionable for agents
using the bundled CLI skill. The command parser remains the owner of syntax;
the automation registry remains the owner of node contracts. Live state and
control remain app-owned. This change does not redesign provider transports,
database migration ownership, or archive storage.

The implementation is on `improve/cli-agent-experience`, based on `a29721c7`.
The audit used built CLI executables, isolated homes, persisted fixtures, and
synthetic local control endpoints. Production state was not used for defect
reproductions.

## Findings and resolutions

| Observed problem | Resolution and evidence |
| --- | --- |
| Most root command families have blank help descriptions; no command discovery schema exists. | Add descriptions and `schema [command path]`, generated from Clap. Integration coverage traverses the complete exposed tree, including the browser's second parser. |
| One automation node requires reading the entire builder registry. | `automation node-types [node]` returns that complete contract; unknown IDs fail. Tests compare the selected node to the original full registry, including unsupported status. |
| Generated JSON spends tokens on indentation. | Compact generated responses across command families; preserve field names, values, envelopes, raw content, human modes, and formatted generated files. |
| Invalid fields succeed for empty lists and can fail only after spawning. | Validate projection before control work and independently of result cardinality. Reject contradictory or ignored output modifiers. |
| Browser target help exits 1 on stderr and contains parser implementation commentary. | Return successful help on stdout before migration or runtime access; use task-facing descriptions and executable recovery examples. |
| Inline JSON can be corrupted by shell/native argument quoting. | `--input` and `--assignments` accept inline objects, `@file`, or piped stdin. Preserve Unicode and quoting; accept a UTF-8 BOM; reject invalid documents before execution. |
| Explicit workspace filtering still applies neighbor filtering; absent operator workspace context broadens to the fleet. | Explicit workspace overrides neighbors. Operator workspace scope uses cwd. Missing managed identities or workspace assignments fail. Resolve topology before display filters; canonicalize existing directories so Windows casing and junctions identify the same workspace. |
| A backend code outside a CLI whitelist becomes `generic`. | Preserve backend code and details; retain established process exit categories. Test present and future codes. |
| Live Inbox/conversation errors can become empty persisted successes. | Fall back only on unavailable endpoints. Preserve rejection, timeout, permission, and malformed-response errors. Add source provenance. Agent show/list follows the same policy. |
| Unknown conversation agent names look like empty histories. | Require a roster match or an existing archive ID; preserve access to archived agents no longer in the roster. |
| Semantic blueprint validation prints `ok: false` but exits 0. | Exit 1 with `validation_failed` and original diagnostics in `error.details`. |
| A matching status received after the wait deadline can succeed; `--next` grants a separate initial-read budget. | Charge IPC, initial reads, and polling to one elapsed-time budget, cap each request and sleep, and reject late matches. Tests exercise delayed reads, cursor continuity, zero budgets, and transport cancellation. |
| Telemetry breakdown silently stops at 24 rows. | Fetch one lookahead row and expose `row_limit` and `truncated`; totals continue to cover the full window. |
| Skill triggers on unrelated terminal work and duplicates conditional rules. | Restrict the trigger to Wardian tasks; keep discovery and shared boundaries in the entrypoint and command details in existing references. |
| Guidance incorrectly requires the app for all writes and promises universal JSON, clone overrides, or single-peer asks. | Document actual offline effects, output exceptions, supported clone flags, fan-out outcomes, bounded observation, and transport-specific guarantees. |

## Compatibility

JSON whitespace is not the automation contract. Field names and values remain
stable, with additive source and truncation metadata. Syntax errors now use
`invalid_arguments`; semantic validation failures are errors rather than
successful reports. Commands that previously ignored output flags now reject
them. Operator default roster scope now matches the documented workspace scope.

Read-side initialization and migration are intentionally retained for legacy
store compatibility and covered by existing migration tests. Static discovery
and help bypass them. Offline access must not be described as universally
read-only. Removing these effects requires a coordinated storage migration
contract, not removal of calls from one CLI path.

Detailed archive and memory reads retain their complete record contracts.
The skill directs incremental observation to watch cursors, bounded tails,
identity projections, flat Library listings, and targeted discovery. It does
not claim that compact serialization bounds record content or execution time.
Native delivery options are documented as transport-specific; a submitted or
timed-out command does not justify automatic replay.

## Measurement contract

Run the same script against a base binary and the changed binary, with the
matching bundled skill checked out for each measurement:

```bash
node scripts/measure-cli-discovery.mjs <cli-binary> <report.json>
```

PowerShell uses the same command with the Windows executable path. The script
uses a fresh isolated home, no managed session, three identical samples per
workload, LF-normalized UTF-8 bytes, and optional `o200k_base` token counts when
Python's `tiktoken` is installed. It measures emitted context, not model
latency. The single-node workload discovers the `task` contract through the
full registry on the base and the targeted command after the change.

| Workload | Base bytes / tokens | Changed bytes / tokens |
| --- | ---: | ---: |
| Full node registry | 10,609 / 2,624 | 6,112 / 1,496 |
| Discover task node | 10,609 / 2,624 | 599 / 153 |
| Root help | 611 / 130 | 1,458 / 291 |
| Agent list help | 461 / 102 | 804 / 172 |
| Skill entrypoint | 6,086 / 1,351 | 3,075 / 642 |

The full registry preserves every field while using 43% fewer tokens; targeted
node discovery uses 94% fewer. Help grows because it now includes descriptions
and valid choices. The skill entrypoint is measured separately by the same
script and reduced by 52% through progressive disclosure.

## Verification

CLI unit and subprocess tests cover emitted JSON, failure exits, field
validation, workspace boundaries, schema traversal, file/stdin JSON, archived
identity resolution, and offline behavior. Synthetic endpoint tests distinguish
live success from semantic, malformed, refused, and timed-out reads. Real
providers are not needed to establish these CLI contracts. Required backend
and documentation verification and an independent reviewer verdict complete
the handoff; no merge or publication is implied by this local branch.

Recorded verification:

This records the CLI audit at its original handoff. The follow-up
[engineering guardrails audit](./2026-09-04-engineering-guardrails.md) resolves
the ambient-log test dependency and makes all-target Clippy mandatory. The
historical failures below are not the current contributor verification policy.

- The final CLI suite passes all 352 tests, including directory casing and junction tests
  first observed failing before the review correction.
- Required workspace Clippy, CLI all-target Clippy, formatting, and workspace
  compilation checks pass on the final code.
- Documentation verification passes. The complete workspace test run passes
  when excluding the single pre-existing test below; the native application
  suite has 1,840 passing tests and 46 explicitly ignored tests.
- The unmodified backend verification command fails on
  `telemetry_real_codex_log::summed_deltas_reproduce_the_providers_cumulative_gauge`.
  This test selects a local provider log, not a versioned fixture. The same
  frozen log fails identically on exact base `a29721c7` and the changed checkout:
  ingested `(381779550, 347558016, 1353940, 715226)` versus provider totals
  `(381275784, 347078400, 1351906, 714004)`. No telemetry parser changes or test
  suppression are included in this branch.
- Independent forward testing completed four discovery, coordination-read,
  node-contract, and PowerShell JSON tasks using the revised skill and isolated
  homes. It did not execute real-provider work.
- A stricter optional `cargo clippy --workspace --all-targets -- -D warnings`
  run fails on unchanged application/core test code. Exact base reproduces the
  same 37 lint diagnostics (plus two compilation-error summaries); these are
  outside the required CI lint command, which does not include `--all-targets`.
- Independent code review closed its workspace-identity blocker after checking
  the correction and red/green evidence, with zero blocking findings remaining.
  Wardian-Reviewer's conversation archive records APPROVED with zero blocking
  findings on 2026-09-04. Its structured reply failed with an OS access-denied
  error, so the archive is the verdict evidence. Its one non-blocking wording
  correction (ordinary syntax exit 1 versus schema lookup exit 2) is applied.

Non-blocking review follow-ups are exposing argument dependencies in syntax
discovery and adding command-level typed-assignment file/stdin cases for more
automation entrypoints. They are not claims of completed functionality and are
tracked separately in [#1121](https://github.com/wardian-app/Wardian/issues/1121).

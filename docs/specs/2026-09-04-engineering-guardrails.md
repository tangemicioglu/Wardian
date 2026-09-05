# Engineering guardrails audit

Tracking issue: [#1123](https://github.com/wardian-app/Wardian/issues/1123).

## Decision and scope

This follow-up starts from merged CLI audit commit `69d9ce54` on
`refactor/engineering-guardrails`. Engineering fixes should leave executable
prevention and task-local guidance, not additional AGENTS.md rules.

At the user's explicit request, this PR also establishes a Wardian-only
publication completion contract in AGENTS.md, with the procedure in
[Pull Request Delivery](../developer/pull-requests.md). Implementation tasks
authorize commit, push, and issue-linked PR delivery unless explicitly
local-only. Zero-blocker review is by local agents; GitHub reviewer requests
are prohibited. This scoped authority contract does not move the engineering
practice catalog into AGENTS.md or change shared skills/class instructions.

The bounded implementation addresses verification infrastructure that agents
copy: Rust test synchronization, ambient provider-log inputs, and the local CI
runner. It does not redesign production ingestion, IPC contracts, or lifecycle
state. Test helpers belong to test-only modules, outside the production API.

## Findings, repairs, and prevention

The owning contributor guide is [Test Reliability](../developer/test-reliability.md).
It maps each failure mode to code and verification rather than maintaining a
second command list here.

- Required Clippy omitted tests and examples. The stricter command reproduced
  37 diagnostics on the base. Repair the test patterns, require all-target
  linting and tests in CI, and retain a separate documentation-test step.
- Tests mixed process-wide blocking locks with async execution, including
  guards hidden inside fixture structs. Sync and async constructors now share
  one Tokio mutex. Remove the obsolete lint suppressions instead of widening
  them. Preserve deliberate contention with an owned thread holder, tested for
  acquisition, release, and cleanup after a panic.
- Default provider tests discovered personal logs, sometimes reread changing
  inputs, and returned success when evidence was absent. Keep deterministic
  fixture assertions in the default suite. Move external-log checks into an
  explicit-path developer example using one owned snapshot. Invalid evidence,
  accounting disagreement, and reparse drift fail rather than skip.
- Missing or empty verification selectors expanded to the full plan; repeated
  selectors were ambiguous. Validate them before reading or executing CI.
  Reject workflow block scalars and indentation mismatches, with negative
  parser fixtures and subprocess exit/output assertions.

## Evidence and boundaries

All-target Clippy passes without warnings after the fixes; the workspace's
`await_holding_lock` suppressions fall from 16 to zero. The full backend suite
passes, including the two seven-test provider suites and five diagnostic
tests. Synthetic append/removal cases prove that a captured log is not reopened.
The fixture subprocess checks poison obsolete ambient overrides without
changing the parent process environment.

The runner's malformed-input cases fail before the repair and pass afterward.
Frontend tests, documentation checks, and the unchanged debt gate verify the
integration. The gate also prevents expanding scattered test-only functions;
the shared lock lives in a dedicated test-only module.

This is not evidence of universal test isolation, real-provider compatibility,
or a Linux/native-browser run. A snapshot is stable after capture, not an
atomic read of a concurrently written source. External diagnostics require a
completed provider log and do not suppress production accounting defects.
Frontend transport validation is a separate design boundary: merely wrapping
untyped JSON in assertions would not make it trustworthy.

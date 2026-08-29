# Base-relative debt budget gate

## Decision

Remove the committed `budgets.json` baseline. `npm run check:budgets` measures
the current checkout and the explicit base revision in a temporary Git
worktree, then fails only when a tracked debt metric increases. The CI workflow
passes the pull request base revision; local runs use `origin/main` when
available and otherwise `HEAD^`.

This preserves the gate's purpose—preventing new debt—without a shared mutable
file that conflicts with concurrent branches or can be accidentally resolved
stale. Metrics that improve or remain equal pass; an already-existing baseline
is not silently treated as fixed.

## Rejected alternatives

- A merge-friendly file would reduce conflict frequency but retain the shared
  mutable baseline and stale-resolution risk.
- Dropping the gate would remove the only automated signal for regressions in
  the debt metrics.

The metric definitions and tracked file paths remain code-owned in
`scripts/verify-budgets.mjs`; only the revision used as the comparison baseline
varies per run.

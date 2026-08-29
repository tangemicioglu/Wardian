# CI-derived local verification

## Decision

`npm run verify:ci` is the project-scoped, fail-fast local verification
harness. Its command sequence is extracted from `# local-verify: <category>`
markers immediately preceding single-line `run:` steps in
`.github/workflows/ci.yml`. The workflow is therefore the only command list;
the runner does not maintain a parallel copy.

The selected steps are the ordinary frontend, backend, and documentation
quality gates. CI-only setup, conditional PR evidence, coverage, security
audits, and platform-specific E2E jobs remain in CI because they need their
declared runner, credentials, or lifecycle environment.

The runner preserves workflow order, uses the literal command text, stops at
the first non-zero result, and prints `FAILED: <command>` for an exact rerun.
`--only frontend`, `--only backend`, and `--only docs` narrow a rerun without
changing the contract.

This harness is verification evidence, not a merge decision and not an
invocation of any model.

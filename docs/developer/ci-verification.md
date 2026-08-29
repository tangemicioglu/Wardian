# Local CI verification

Run the project-owned core CI checks with:

```sh
npm run verify:ci
```

To rerun one category after a failure:

```sh
npm run verify:ci -- --only backend
```

The runner reads the marked `run:` steps from `.github/workflows/ci.yml`, so
the command list and flags stay coupled to CI. It fails fast and echoes the
literal failing command. Use `npm run verify:ci -- --list` to inspect the
resolved sequence without executing it.

The local sequence covers the frontend, backend, and documentation quality
steps. PR screenshot/code-claim checks, dependency audits, coverage uploads,
and browser/native suites remain CI- or environment-specific.

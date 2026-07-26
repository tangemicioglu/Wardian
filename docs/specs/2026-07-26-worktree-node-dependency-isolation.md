# Worktree Node Dependency Isolation

- **Status:** Implemented
- **Date:** 2026-07-26

## Context

Wardian previously linked every managed Node worktree to the source checkout's
`node_modules` directory. That avoided installing dependencies in every
worktree, but it was unsafe when a worktree's dependency manifest or lockfile
differed from its source checkout.

## Decision

Wardian shares `node_modules` only when `package.json`, `package-lock.json`,
and `npm-shrinkwrap.json` are identical in the source checkout and worktree.
Missing manifests match only when absent from both locations.

When any dependency manifest differs, Wardian removes only its own generated
`node_modules` link. It does not create a replacement dependency folder and
does not modify either manifest. A local dependency directory exists only if a
person or agent intentionally installs dependencies in that worktree.

## Consequences

- Worktrees with the same dependency graph continue to share one install.
- Worktrees with different locks cannot read or mutate another branch's
  dependency tree.
- Wardian avoids automatically creating one `node_modules` folder per
  worktree.

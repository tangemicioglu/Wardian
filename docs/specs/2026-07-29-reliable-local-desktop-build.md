# Reliable Local Desktop Build

## Context

The documented local production build is `npm run tauri build`. It must work from a regular terminal session without a running Wardian app, an agent session, or a shell-specific environment setup.

The release profile previously enabled Rust incremental compilation to reduce repeated build time. On Windows, stale release incremental artifacts after a Rust toolchain change could instead crash `rustc` with `STATUS_ACCESS_VIOLATION`. A Tauri `beforeBuildCommand` cannot correct this because it runs in a child process and cannot modify the environment of Tauri's later Cargo invocation.

## Decision

Set `incremental = false` explicitly in the workspace `[profile.release]`. Tauri reads this Cargo configuration for the ordinary local build, so the existing `npm run tauri build` command remains the complete interface on every platform.

## Consequences

- Local release builds prioritize deterministic compiler inputs over incremental rebuild speed.
- Development builds retain their existing development-profile settings.
- No wrapper command, persistent environment variable, cache deletion, or Wardian runtime is required for a local desktop build.

## Verification

From the repository root, run:

```bash
npm run tauri build
```

The command must produce the normal local desktop bundle without requiring a running Wardian session.

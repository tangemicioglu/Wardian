# Wardian Product Direction

Wardian is a local-first desktop habitat for live agents, automations, reusable
context, and durable evidence. The early phase-based roadmap has served its
purpose; major foundations such as the multi-view shell, PTY-backed agents,
the Wardian CLI, worktrees, automations, Queue evidence, Graph topology, remote
mobile control, and package-manager distribution are now part of the product.

This roadmap describes current product direction without promising a strict
phase sequence. Priorities can move as provider CLIs, release infrastructure,
and user automations change.

## Current Priorities

### Runtime Reliability and Provider Fidelity

- Keep real PTY behavior stable across Windows ConPTY, macOS, and Linux.
- Preserve provider-specific launch policy, delivery evidence, transcript
  capture, status transitions, and terminal rendering as provider CLIs change.
- Make failures inspectable through Queue, automation history, conversation
  archives, logs, and native E2E evidence.

### Reusable Context and Capabilities

- Continue making the Library the home for prompts, skills, classes, automation
  blueprints, and future MCP definitions.
- Keep reusable artifacts filesystem-backed and inspectable under the Wardian
  home instead of hiding them in opaque app state.
- Expand safe deployment and synchronization of skills and context across
  global, class, agent, automation, workspace, and team scopes.

### Coordination Surfaces

- Improve Graph topology as the visible control surface for agent-to-agent
  communication boundaries.
- Tighten teams, watchlists, structured asks/replies, CLI coordination, and
  Queue triage so multi-agent work remains bounded and reviewable.
- Preserve operator control over manually drawn connections, team-seeded
  topology, and workspace fallback behavior.

### Automation Operations

- Harden automation authoring, validation, launch dialogs, schedules, run
  observation, history, and failure records.
- Keep the Rust automation engine deterministic and testable while exposing
  higher-level automation ergonomics in the desktop app and CLI.
- Make automation outcomes easy to review, reuse, and promote into durable
  context.

### Remote, Packaging, and First-Run Polish

- Improve the mobile remote PWA while keeping the desktop app the authority for
  agents, PTYs, provider CLIs, filesystem access, and automation execution.
- Keep installer, updater, winget, Homebrew, APT, `.deb`, and AppImage guidance
  aligned with actual release artifacts.
- Reduce first-run friction through better provider readiness checks,
  troubleshooting, and cross-platform documentation.

## Later Directions

- Richer Garden spatial organization and persistent workspace layouts.
- MCP configuration and deployment from the Library.
- Remote Queue hydration from desktop-owned Queue storage.
- Broader package-manager coverage after sandbox and permission tradeoffs are
  understood.
- More automation around file-watch, listener, and project-context automations.

## What Is Not a Roadmap Contract

This document is not a release schedule. Historical specs in `docs/specs/`
remain useful design records, but their phase names and deferred sections
should not be read as the current delivery order. Use the current README,
public docs, changelog, and open issues for up-to-date release context.

# Design Consistency Audit

**Status:** Implemented

## Purpose

This audit makes repeated Wardian concepts read and behave the same wherever
they appear. It is deliberately limited to concrete user-facing mismatches
found in the current desktop and remote surfaces; it does not rename backend
commands, persisted fields, or provider protocol values.

## Findings and decisions

| Finding | Evidence | Decision |
| --- | --- | --- |
| The primary agent surface is called `Agents` in the workbench, but its Settings category and card preference still say `Grid`. | `coreSurfaceRegistry.ts` contributes `Agents`; `SettingsModal.tsx` used `Grid` and `Grid card display`. | Use **Agents** for the settings category and **Agent card display** for the preference. The Grid/Single controls remain layout modes within that surface. |
| The agent-management pane alternates between `Agent Configuration` and the abbreviated `Agent Config`. | The sidebar rail used the full term; `SidebarContentPane` used the abbreviation. | Use **Agent Configuration** for the generic pane. `Configure Agent` remains the specific action for one existing agent. |
| Creating an agent is described as `Spawn Agent`, but the submit button says `Initialize`. | The spawn panel heading and back action use Spawn; its primary action used Initialize. | Use **Spawn Agent** and **Spawning...** for the agent-creation flow. `Initialize Repository` remains a distinct Source Control action. |
| The same provider choice is called `Provider Engine` in agent forms and `Default provider` in Settings. | Spawn, configure, and clone forms used `Provider Engine`; app settings used `Default provider`. | Use **Provider** for a per-agent choice and **Default provider** only for the application-wide default. |
| Equivalent agent states use mixed raw provider strings and inconsistent status treatment. In particular, desktop processing text used the gold accent despite the semantic cyan contract, and desktop errors could fall back to gray while remote errors were red. | `statusUtils.ts`, `remoteAgentStatus.ts`, `graphProjection.ts`, and the status-color guidance in `DESIGN.md`. | Normalize known agent status variants before presentation, use the same semantic tone for desktop, remote, and Graph, and display canonical labels: Idle, Processing, Action Required, Pending, Off, Headless, Restoring, and Error. |

## Implementation boundaries

- Backend statuses and persisted values remain compatible. Normalization occurs
  at the presentation boundary.
- Workflow run states remain distinct from agent lifecycle states. For example,
  a workflow can be `Awaiting approval` while an agent can be `Action Required`.
- Existing internal identifiers such as `grid_card_display_mode` remain stable
  to avoid breaking saved settings.

## Validation

- Focused unit tests cover terminology, status normalization, tone mapping, and
  the affected agent forms and Settings panel.
- The frontend typecheck and relevant browser regression suites verify that the
  renamed controls remain reachable through the desktop and remote interfaces.

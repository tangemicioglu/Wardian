# Guided Tour Habitat Setup

## Decision

Replace the abstract Guided tour slides with a practical, four-step setup path:

1. Configure and create an Evolver through focused name, class, workspace,
   provider, and spawn controls.
2. Prompt the Evolver to create an Orchestrator through Wardian's CLI.
3. Verify the pair's connection in Graph.
4. Schedule a conversation-pattern review on the Evolver.

The tour stays optional, non-blocking, and user-driven. On a new habitat, the
app first offers **Take the tour** or **Not now**; existing persisted
onboarding files are treated as opted out so an upgrade never interrupts a
working habitat. Each step focuses its real surface with a dimming spotlight
and advances only when the corresponding persisted state exists. The tour
never spawns an agent, edits topology, or creates a schedule itself. The
Evolver's explicit user-prompted CLI action creates only the peer; the user
creates the visible relationship in Graph. Provider, workspace, archive scope, and cadence
are operator decisions.

The tutorial must distinguish the Wardian home from an agent workspace. The
Wardian home stores habitat state and is made available to the agent so its CLI
can control the running app. The peer spawn must instead use the Evolver's
explicit current workspace; an omitted workspace falls back to the operating
system user directory and must not be presented as a Wardian-home default.

## Conversation Pattern Review Sample

Ship an editable `conversation-pattern-review.md` workflow sample with the
existing bundled samples. It assigns its review task to `role:evolver`, uses a
manual trigger so the operator can test it before scheduling, and reports
recommendations only. The prompt requires bounded use of Wardian's conversation
archive interface and explicitly prohibits recursive agent-directory scans,
automatic changes, or external actions.

The user binds `role:evolver` to their own Evolver in the schedule dialog and
chooses a separate background conversation and cadence. The sample is copied
only when missing, preserving user edits and preventing automatic schedule
creation.

## Visual Language

The tour uses sentence case for its progress label and step titles. A spotlight
leaves every workbench control interactive while dimming the rest of the
workbench, so it never prevents a tab change or another needed navigation
action. The guide places itself below the tab strip and chooses a corner that
does not overlap its focused control. The Evolver setup uses Next field between
its form controls and waits only for the final spawn action. The Settings entry
is a review mode that starts at the first area and provides Next area
navigation, so an existing habitat can revisit every surface. The peer-creation
step includes a bounded, copyable prompt that visibly names the Wardian CLI
actions the Evolver will use.

## Verification

- Unit coverage verifies the first-launch choice and the action-gated Evolver
  path.
- Browser coverage accepts the first-launch choice, captures the creation
  spotlight, and advances when the test roster gains an Evolver.
- Backend coverage parses and validates the bundled workflow sample.

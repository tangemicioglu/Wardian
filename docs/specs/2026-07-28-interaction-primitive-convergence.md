# Interaction Primitive Convergence

**Status:** Implemented foundation; progressive migration

## Purpose

Wardian's workbench, configuration, agent, workflow, and library surfaces
previously styled repeated interactions independently. This specification makes
the shared visual contract explicit while preserving the different information
shapes of a filesystem tree, source-control resource list, workflow table, and
run-history card.

## Shared contract

| Primitive | Contract | Current migration |
| --- | --- | --- |
| Dialog | `wardian-dialog-overlay` uses the semantic overlay token. `wardian-dialog-panel` provides the surface, border, shadow, and compact/standard/wide sizing. Header and footer separators use shared classes. | Settings, confirmation, dirty-surface, workbench-conflict, custom-clone, and workflow-launch dialogs. |
| Button | `wardian-button` defines a 32px form-action baseline with secondary, primary, and danger variants. `wardian-icon-button` is the compact icon counterpart. | Dialog and workflow-launch actions; destructive confirmation now uses semantic danger contrast. |
| Segmented control | `wardian-segmented-control` groups mutually exclusive options without creating a separate button language. | Workflow run/schedule selector. |
| Menu | `wardian-menu` defines the shared surface. Menus use `menu`/`menuitem` semantics, focus the first item, support Arrow Up/Down, Home/End, and Escape, and restore prior focus. | Generic context menu, workbench context menu, library creation menu, and schedule action menu. |
| Agent status | Lifecycle labels and semantic tones come from `statusUtils`; transient thought text is Activity, not Status. | Watchlist, Dashboard, Graph, Garden, desktop remote lists, and Graph canvas tooltip. |
| Inbox event status | Event labels and outcome tones are shared through `queuePresentation`; Headless violet is never used for a completed event. | Desktop and remote Inbox. |
| Git/workflow status | Git status labels/colors and workflow run status tokens have shared helpers. | Explorer + Source Control, run history + schedules + Garden workflows. |

## Audit ledger

| Finding | Resolution |
| --- | --- |
| Dialogs used competing overlays, panels, radii, and inline styles. | Shared dialog overlay/panel/header/footer classes now cover the identified modal families. |
| Equivalent buttons used unrelated sizing and colors. | Shared primary, secondary, danger, and segmented-control button variants cover the migrated dialogs and workflow launcher. |
| Token use was not consistently semantic. | Added semantic contrast tokens; centralized Inbox, agent, Git, and workflow colors; removed hard-coded confirmation white, custom-clone black overlay, and sidebar black badge text. |
| Trees, resources, tables, and activity cards had inconsistent density. | Classified their distinct information shapes and documented the primitives; their shared interaction behavior now has an explicit migration rule. |
| Toolbars mixed unrelated control styles. | Defined the compact toolbar rule while retaining the navigation rail as a deliberately distinct control. |
| Settings form conventions were private to Settings. | Made Settings the explicit reference contract and migrated adjacent dialog controls to the shared dialog/button language. |
| Menus had separate surfaces and inconsistent keyboard behavior. | Shared menu surface now applies to generic, workbench, library, and schedule menus; generic menus gained keyboard navigation and focus return. |
| Completed workflow Inbox items appeared violet like Headless agents. | Inbox presentation now follows event outcome: completion is green, attention amber, active work cyan, failure red. |
| Garden relied on color, pulse, and pips without a textual status cue. | Added a canonical visible legend, live selected-unit summary, canvas description, and shared status colors. |
| Watchlist Status showed a transient thought while Graph showed lifecycle state. | Status is canonical; transient work is exposed as Activity in a tooltip/accessible label. |
| Agent, Inbox, and workflow approval language used conflicting casing. | Agent state displays Action Required, Inbox event displays Action required, and workflows display Awaiting approval. |
| Workflow monitor and schedule labels used lowercase beside title-case status labels. | Monitor stats and schedule labels now use title case. |
| The workflow glance chip said Need without context. | Renamed to Attention. |
| Generic collection copy alternated among roster and All Agents. | Live generic labels now use Agent List; watchlist remains the saved filtered collection. |
| A test-only placeholder retained Queue and Workflow Builder legacy copy. | Removed the unreferenced component and its test. |

## Information-density rules

- `TreeRow` behavior is reserved for hierarchical filesystem navigation.
- `DenseResourceRow` behavior is reserved for source-control resources and
  exposes actions on hover/focus.
- `DataTable` behavior is reserved for structured, comparable schedule data.
- `ActivityCard` behavior is reserved for run summaries and event history.

These are intentional different layouts. Shared focus, status placement,
hover, selected, and action-reveal behavior should be added through primitives
without forcing trees and tables into the same visual geometry.

## Form and toolbar migration rule

Settings remains the reference for configuration row hierarchy. New or touched
forms should use the shared dialog/button contract and align labels, controls,
help text, and inline status to Settings before adding feature-local styles.
Pane toolbars use compact actions; the icon rail remains a navigation control,
not the default form-button style.

## Accessibility requirements

- Dialogs expose `role="dialog"`, `aria-modal`, a name, Escape where dismissal
  is permitted, and a visible focus target.
- Menus expose `menu`/`menuitem` semantics and preserve keyboard return focus.
- Garden never relies on color, pulse, or pips alone: it has a visible canonical
  legend and a live selected-unit status summary.
- Status columns show canonical state; transient work appears as Activity in a
  tooltip or secondary field.

## Follow-up boundaries

The shared CSS contract intentionally avoids a disruptive one-pass rewrite.
When a dialog, menu, form, or toolbar is materially edited, migrate it to these
primitives and add feature-specific visual evidence. Extract React wrappers
only after at least three callers need identical behavior beyond the current
class-and-accessibility contract.

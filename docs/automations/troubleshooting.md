# Automation Troubleshooting

This page is for diagnosing the most common automation issues from a user perspective before diving into backend internals.

## A Scheduled Automation Did Not Run Immediately

This is usually expected.

Interval-based scheduled automations do not fire at creation time. They schedule the first run in the future based on the selected interval or wall-clock time.

Check:

- whether the blueprint was invoked by a schedule instead of a manual run
- whether the schedule card shows `Live` with a future next-run time
- whether you expected a listener but actually created a scheduled task

## A Scheduled Task Looks Stuck on Due

A `Due` state means the task is ready or overdue relative to the current clock.

Check:

- whether the task is actually running right now
- whether it is a one-time schedule that should disappear after completion
- whether the scheduler is active and the app is still open

If the behavior seems wrong after that, inspect the developer-side scheduler notes in [Automation Engine Architecture](../developer/automation-engine.md).

## Pause and Resume Changed the Timing

Current expected behavior is that pause/resume preserves remaining time.

If the task appears to restart a full interval after resume, treat that as a regression rather than normal behavior.

## The Run Modal Appears When I Did Not Expect It

Wardian opens the modal when launch-time data is needed.

Check whether the automation has:

- agent roles that need assignment
- a Manual Trigger input schema
- both of the above

If yes, the modal is working as designed.

## My Automation Did Not Start From an Event

Event-driven invocation is an invoker integration, not a blueprint trigger node.

Expected behavior:

- event sources create normal durable runs with their event payload
- scheduled invocations do not become listeners
- manual automations run immediately

## A Scheduled Task Was Deleted but the Automation Still Exists

This is expected.

Deleting a schedule removes the **scheduled instance**, not the automation definition. The automation remains available in the builder and library unless you delete the automation itself.

## I Created Two Schedules With the Same Automation Name

This is also expected.

Wardian allows multiple scheduled instances of the same automation. Distinguish them by:

- target summary
- role mappings
- schedule timing

## A Node Exists in the Model but Not in the Builder

The automation model may include reserved node types for forward compatibility.

`sub_automation` is currently registered as unsupported because it lacks a durable
child-run contract. Validation reports `unsupported_node_type`, and the Builder
does not offer it. Confirm the support level in [Node Reference](./node-reference.md)
before assuming an exported node is executable.

## A Branch Condition Always Took the False Path

Branch and loop conditions currently accept only a dot-separated registry path,
such as `nodes.agent-1.output.ready`. Expressions such as `===`, `&&`, or
quoted literals are rejected with `invalid_condition`; they are not evaluated as
JavaScript or another expression language.

Use the run registry values produced by prior nodes, or remove the comparison
until a versioned condition language is available.

## Where to Go Next

For deeper debugging:

- user behavior and launch rules: [Triggers](./triggers.md)
- scheduling behavior: [Scheduled Runs](./scheduled-runs.md)
- node capabilities: [Node Reference](./node-reference.md)
- backend execution details: [Automation Engine Architecture](../developer/automation-engine.md)
- builder internals: [Visual Builder Architecture](../developer/visual-builder.md)

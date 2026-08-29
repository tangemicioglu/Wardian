# Triggers

Triggers decide how an automation enters the runtime.

In Wardian, every automation run enters through an invoker. A launch can:

- run immediately
- create a scheduled task
- activate a live listener

The blueprint's `manual_trigger` supplies the run input; the invoker decides
whether that input comes from a user, a schedule, or a future event source.

## Manual Trigger

Use **Manual Trigger** when you want an on-demand automation.

Best for:

- testing an automation from the builder
- ad hoc automations
- automations that should only run when a user explicitly starts them

Behavior:

- launching the automation starts a run immediately
- if the manual trigger defines an input schema, the run modal asks for those values first
- if the automation also contains agent roles, the same modal can collect agent assignments

## Scheduled Invocations

Use a schedule when you want Wardian to create repeated or delayed invocations
of a saved blueprint. Scheduling is an invoker, not a node in the blueprint.

Best for:

- recurring reviews
- timed maintenance tasks
- delayed one-time automations
- repeating agent routines

Behavior:

- schedules reference a saved blueprint and validate it before creation
- a scheduled invocation supplies the same input/binding contract as a manual run
- launching a scheduled automation does **not** create a live listener
- an automation can have multiple scheduled task instances at the same time

If the automation contains agent nodes or a manual input schema, the invoker
provides those values before creating the run.

### Schedule Types

The current scheduled trigger supports:

- **Minutes**
- **Hours**
- **Daily**
- **Weekly**
- **One-Time**

User-visible timing rules:

- interval schedules such as `Minutes` and `Hours` schedule the first run **after** the interval elapses, not immediately
- `Daily` and `Weekly` wait for the next matching wall-clock time
- `One-Time` runs once at the specified datetime and then disappears after completion

## Future Event Invocations

File watching and webhook-style launches are future invoker integrations. They
must supply the same `input`, `bindings`, provider, and workspace boundary as a
manual or scheduled invocation; they are not additional trigger node types.

Planned behavior:

- an event produces a normal durable automation run with its event payload
- listener lifecycle belongs to the invoker, not the blueprint graph

Use listener triggers for:

- file-change automation
- event-driven automations that should keep watching for input

Do not model an event source as a fake trigger node. Keep the event-source
implementation at the invoker boundary.

## Launch Surface Differences

The trigger type matters more than the button you clicked, but the surface still affects the flow:

- **Builder**: saves current canvas state first, then launches a manual run
- **Library/CLI**: launches the saved automation through the same run contract
- **Monitoring sidebar**: acts on existing durable runs and invoker instances

## Practical Rule of Thumb

- want one run right now: use **Manual Trigger**
- want repeated or delayed runs: create a **schedule** for the blueprint
- want an event-driven run: use an event invoker when that integration is available

## Related References

- [Scheduled Runs](./scheduled-runs.md)
- [Agent Assignment](./agent-assignment.md)
- [Automation Engine Architecture](../developer/automation-engine.md)

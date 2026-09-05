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

The scheduler currently supports:

- **Minutes**
- **Hours**
- **Daily**
- **Weekly**
- **One-Time**

User-visible timing rules:

- interval schedules such as `Minutes` and `Hours` schedule the first run **after** the interval elapses, not immediately
- `Daily` and `Weekly` wait for the next matching wall-clock time
- `One-Time` runs once at the specified datetime and then disappears after completion

## Listener Invocations

Use a listener when an external event should start the automation. Listeners are
invokers, not blueprint nodes: the event source lives at the invoker boundary
and supplies the same `input`, `bindings`, provider, and workspace contract a
manual or scheduled invocation does.

Three kinds ship today:

- **File change** - a matching file under a watched path was created, modified,
  or removed.
- **Inbound webhook** - an authenticated HTTP request arrived at `/hooks/<path>`.
  Use this for a system you administer.
- **Web poll** - a watched URL's response changed. Use this for a system you do
  not administer, such as a release feed for a project you merely depend on.

Behavior:

- an event produces a normal durable automation run carrying its payload
- listener lifecycle belongs to the invoker, not the blueprint graph
- a burst of file events collapses into one run
- a retried webhook delivery resolves to the run it already created
- file and webhook events that occur while Wardian is closed are lost; only a
  poll detects a change that happened during downtime

Do not model an event source as a trigger node. Keep the event-source
implementation at the invoker boundary.

See [Listeners](./listeners.md) for setup, payload fields, overlap policy, and
diagnosis.

## Launch Surface Differences

The trigger type matters more than the button you clicked, but the surface still affects the flow:

- **Builder**: saves current canvas state first, then launches a manual run
- **Library/CLI**: launches the saved automation through the same run contract
- **Monitoring sidebar**: acts on existing durable runs and invoker instances

## Practical Rule of Thumb

- want one run right now: use **Manual Trigger**
- want repeated or delayed runs: create a **schedule** for the blueprint
- want a run when a file changes, a system posts to you, or a web resource
  changes: create a **listener**

## Related References

- [Scheduled Runs](./scheduled-runs.md)
- [Listeners](./listeners.md)
- [Agent Assignment](./agent-assignment.md)
- [Automation Engine Architecture](../developer/automation-engine.md)

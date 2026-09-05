# Automations

Wardian automations are reusable automations built from nodes, connections, and runtime assignment data. A visual builder defines the graph, while the Rust automation engine executes it through a deterministic candidate-node loop with pulses, registry updates, and telemetry events. This section is the main user-facing reference for building automations, understanding how they launch, and managing scheduled or live automation behavior over time.

Automations are editable process artifacts, not sealed automations. The saved
template describes the reusable process, while each run produces durable
evidence that can be reviewed, compared, attached to a team/project context, or
preserved as memory-ready context. This keeps automation malleable: users can
start with a manual sequence, turn it into an automation, then refine the graph and
runtime assignments as the work changes.

## Automation Mental Model

An automation has four layers:

- **Template**: the saved graph of nodes, edges, and automation settings.
- **Launch behavior**: whether that template runs manually, creates a scheduled task, or starts a live listener.
- **Runtime state**: active runs, scheduled instances, listener status, node outputs, and role mappings.
- **Engine loop**: the internal candidate-node loop that decides which nodes are ready, consumes dependency pulses, executes each node, and emits run telemetry back to the UI.

In practice, you usually move through automations in this order:

1. Build or edit the graph in the Automation view.
2. Save and launch it from either the main builder or the sidebar library.
3. Monitor active runs, live listeners, and scheduled tasks from the automation sidebar.

## Start Here

- **[Building Automations](./building-automations.md)**: Use the canvas, block library, node settings, and variable assistant.
- **[Automation Samples](./samples.md)**: Start from editable, privacy-safe templates for common automation patterns.
- **[Triggers](./triggers.md)**: Understand manual runs, scheduled invocations, and live listeners.
- **[Scheduled Runs](./scheduled-runs.md)**: Manage scheduled task instances, pause/resume, run now, and deletion.
- **[Listeners](./listeners.md)**: Start a run from a file change, an inbound webhook, or a change to a watched web resource.
- **[Node Reference](./node-reference.md)**: Reference every current automation node type and its user-visible behavior.
- **[Agent Assignment](./agent-assignment.md)**: Learn how roles, direct agent selection, and the run modal work.
- **[Troubleshooting](./troubleshooting.md)**: Diagnose the most common automation problems quickly.

## Automation Surfaces

Wardian exposes automations in four main surfaces:

- **Automation Builder**: best for authoring, wiring, configuring, and testing automations.
- **Automation Library**: best for launching saved automations quickly without opening the canvas.
- **Active Monitoring**: best for watching active runs, live listeners, and scheduled task instances.
- **Wardian CLI**: best for agents and automation to list, show, run, and stop automations through the running desktop app.

## What This Section Covers

This automation section focuses on user-visible behavior:

- what each node is for
- when a run modal appears
- how scheduled tasks behave
- what happens when an automation is launched from different surfaces
- how agent assignments affect execution
- where automation outcomes can surface after a run completes

For backend implementation details, see:

- [Automation Engine Architecture](../developer/automation-engine.md)
- [Visual Builder Architecture](../developer/visual-builder.md)

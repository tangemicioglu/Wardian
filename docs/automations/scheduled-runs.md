# Scheduled Runs

A scheduled run is a runtime instance created by a schedule (an invoker) for a
saved automation blueprint. Scheduling is separate from the automation
definition and is not represented by a blueprint node.

This distinction matters because Wardian supports multiple scheduled instances of the same automation at the same time.

## What a Scheduled Task Stores

Each scheduled task instance keeps track of:

- the source automation
- the schedule definition
- the assigned agents or role mappings
- whether the task is paused
- the next expected run time
- any remaining delay if the task was paused mid-countdown

That means you can schedule the same automation more than once with different targets without overwriting the original automation template.

## Creating Scheduled Tasks

Scheduled tasks can be created from either launch surface:

- **Automation Builder**: saves the automation first, then creates the schedule instance
- **Automation Library**: creates the schedule instance from the saved automation

If an automation needs launch-time input or agent assignment, the run modal appears before the schedule is created.

## Sidebar Statuses

Scheduled tasks appear in the sidebar under **Scheduled Tasks**.

The current card statuses are:

- **Live**: active and waiting for the next run time
- **Paused**: temporarily stopped
- **Due**: the task should run as soon as the scheduler processes it
- **Running**: a scheduled run is currently executing

## Expanding and Acting on a Scheduled Task

The scheduled task row expands inline to show more detail, including:

- schedule summary
- current status
- next run timing
- target summary
- role-to-agent mappings when present

Available actions:

- **Pause / Resume**
- **Run Now**
- **Edit Automation**
- **Delete Schedule**

The same actions are also available from the schedule context menu.

## Pause and Resume Behavior

Pause and resume preserve the remaining timer.

That means Wardian does **not** restart the full interval from scratch when you resume a repeating schedule. If a task had 17 minutes left when paused, it resumes with roughly 17 minutes left.

## Run Now

**Run Now** launches the automation immediately from the scheduled task instance.

For repeating schedules, Wardian also re-arms the next scheduled execution after the immediate run.

## One-Time Schedules

One-time schedules are temporary by design.

Behavior:

- they wait until the specified datetime
- they execute once
- after the run completes, the scheduled task is removed from the sidebar instead of remaining as overdue state

## How Targets Are Shown

Scheduled task cards summarize their target using the most specific information available:

- assigned agents from runtime role mappings
- direct agent IDs configured on agent nodes
- roles, if no concrete agent is assigned yet
- `Unassigned` when no target can be resolved

This is why two scheduled instances of the same automation can share the same automation name but still show different targets in the sidebar.

## Related References

- [Triggers](./triggers.md)
- [Agent Assignment](./agent-assignment.md)
- [Troubleshooting](./troubleshooting.md)

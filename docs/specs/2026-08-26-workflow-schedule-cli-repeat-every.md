# Workflow schedule CLI recurrence interval

## Decision

The schedule CLI exposes the existing `ScheduleDefinition.repeat_every` field
as `--repeat-every <positive-integer>`. This field represents a weekly interval
in weeks, matching the shared Rust model and the Workflows schedule editor.

New `--weekly` schedules default to `repeat_every: 1`, so the ordinary weekly
command is valid without an additional flag. On update, omitting the option
preserves an existing weekly interval; supplying it changes that interval and
may be done without repeating the weekly day/time expression.

The supported range is 1 through 520 weeks (roughly ten years). This ceiling
keeps the day-based scheduler projection bounded and rejects extreme values
before `schedules.json` is replaced.

`--every` remains the interval cadence in minutes. `repeat_every` is rejected
for interval, daily, monthly, specific-date, and one-time cadences because the
current shared schedule model does not apply that field to them. Values must
be positive; invalid values must fail before `schedules.json` is replaced.

## Verification contract

The CLI integration suite invokes the compiled command with an isolated
`WARDIAN_HOME` and checks the persisted schedule through `workflow schedule
list`. It covers default weekly creation, an explicit weekly interval, update,
zero, malformed, and out-of-range values, the upper boundary, and the original
weekly command shape with typed assignments. Core scheduling tests cover the
same bound before the scheduler's search window is calculated.

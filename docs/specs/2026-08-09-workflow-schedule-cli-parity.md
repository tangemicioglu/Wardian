# Workflow schedule CLI parity

## Decision

The workflow schedule CLI is the disk-backed control surface for the same
schedule contract exposed by the Workflows launch dialog and Monitor. Every
schedule field that the UI can edit must be representable by `wardian workflow
schedule add` or `wardian workflow schedule update`.

The schedule workspace is required. A scheduled run must have a stable,
existing directory before it is saved; falling back to the run directory made
failures depend on where the scheduler happened to launch. Existing persisted
schedules without a workspace remain readable for compatibility, but an update
must supply a workspace before that schedule can be saved again.

## CLI contract

Create a schedule with one cadence option:

```text
wardian workflow schedule add --blueprint <id> --name <name> \
  --workspace <existing-directory> \
  (--every <minutes> | --daily <HH:MM> | --weekly <days@HH:MM> |
   --monthly <days@HH:MM> | --specific-dates <dates@HH:MM> | --at <datetime>)
```

The cadence forms match the Schedule editor:

- `--every 60`
- `--daily 09:30`
- `--weekly Mon,Wed,Fri@09:30`
- `--monthly 1,15@09:30`
- `--specific-dates 2026-09-01,2026-09-15@09:30`
- `--at 2026-09-01T09:30`

Recurring schedules can use `--end never`, `--end on_date --end-date
YYYY-MM-DD`, or `--end after_occurrences --max-occurrences N`.

Update changes only the options supplied and retains the schedule id, run
occurrence count, last-run status/error/timestamp, input, assignments, and
other configuration that was not selected for change:

```text
wardian workflow schedule update <schedule-id> \
  --name <new-name> --daily 09:30 --workspace <existing-directory>
```

`--assignments` accepts the typed role map used by the UI. `--bind role=value`
remains available as a shorthand for an agent id or known provider. Assignment
workspace values are validated as existing directories before persistence.

Pause, resume, run-now, remove, and list continue to operate on the persisted
schedule id. All add and update validation occurs before the atomic schedules
file replacement, so a rejected blueprint, cadence, provider, assignment, or
workspace cannot leave a partially updated schedule document.

## UI behavior

The launch dialog requires a workspace in Schedule mode and calls
`schedule_update` when editing an existing row. Editing no longer removes and
recreates the schedule, so Monitor identity and execution history remain
stable.

## Compatibility

The legacy optional-workspace behavior remains available only to old persisted
records while they are being repaired. New schedules and all successful edits
use a canonical absolute workspace path.

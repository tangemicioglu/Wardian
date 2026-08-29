# Automations

Use automation commands to inspect, validate, normalize, and run automation
blueprints:

```bash
wardian automation node-types
wardian automation validate <path-to-automation.md>
wardian automation parse <path-to-automation.md>
wardian automation normalize <path-to-automation.md> --write
wardian automation exec <path-to-automation.md>
wardian automation runs
wardian automation run-show <blueprint-id> <run-id>
wardian automation replay <blueprint-id> <run-id>
wardian automation schedule add --blueprint <id> --name <name> \
  --workspace <existing-directory> --every 60
wardian automation schedule update <schedule-id> \
  --workspace <existing-directory> --daily 09:30
wardian automation schedule add --blueprint <id> --name <name> \
  --workspace <existing-directory> \
  --weekly Mon,Wed,Fri@09:30 --repeat-every 2
wardian automation schedule list
```

`validate`, `parse`, `normalize`, `runs`, `run-show`, and `replay` are
disk-backed. `exec` and schedule actions that launch runs require the desktop
app for the same `WARDIAN_HOME`.

Bundled, editable examples live in `<WARDIAN_HOME>/library/automations/samples/`.
They are templates only: inspect and adapt one before running it, and create a
schedule only after the user explicitly asks for one.

Scheduled schedules require an existing workspace directory. `schedule update`
edits the persisted record in place, so its id and run history remain stable.
Weekly schedules default to a one-week recurrence; use
`--repeat-every <positive-integer>` for a weekly interval from 1 through 520
weeks. The
interval-only `--every` option remains expressed in minutes, and the current
schedule model does not support repeat intervals for other calendar cadences.

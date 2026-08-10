# Workflows

Use workflow commands to inspect, validate, normalize, and run workflow
blueprints:

```bash
wardian workflow node-types
wardian workflow validate <path-to-workflow.md>
wardian workflow parse <path-to-workflow.md>
wardian workflow normalize <path-to-workflow.md> --write
wardian workflow exec <path-to-workflow.md>
wardian workflow runs
wardian workflow run-show <blueprint-id> <run-id>
wardian workflow replay <blueprint-id> <run-id>
wardian workflow schedule add --blueprint <id> --name <name> \
  --workspace <existing-directory> --every 60
wardian workflow schedule update <schedule-id> \
  --workspace <existing-directory> --daily 09:30
wardian workflow schedule list
```

`validate`, `parse`, `normalize`, `runs`, `run-show`, and `replay` are
disk-backed. `exec` and schedule actions that launch runs require the desktop
app for the same `WARDIAN_HOME`.

Bundled, editable examples live in `<WARDIAN_HOME>/library/workflows/samples/`.
They are templates only: inspect and adapt one before running it, and create a
schedule only after the user explicitly asks for one.

Scheduled schedules require an existing workspace directory. `schedule update`
edits the persisted record in place, so its id and run history remain stable.

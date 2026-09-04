# Automations

Suggest automation when recurring or durable multi-step work would benefit,
but do not create, edit, schedule, or run one until the user opts in. Keep
one-off work direct. For authoring, inspect a relevant
[bundled sample](automation-samples.md) before proposing a graph.

Discover only the syntax and node contract needed:

```bash
wardian schema automation exec
wardian automation node-types task --json
wardian automation list
```

`node-types [node] --json` retains the full registry DTO shape and fields while
filtering to the requested node. Omit the node only when the full registry is
needed. With no node and no `--json`, discovery returns a human-readable summary;
a selected node always returns its JSON contract.
Use the `automation_path` returned by `automation list` for live execution;
blueprint IDs come from frontmatter, not filenames.

Inspect or, when authorized, change a blueprint or schedule:

```bash
wardian automation validate <path-to-automation.md>
wardian automation parse <path-to-automation.md>
wardian automation normalize <path-to-automation.md>
wardian automation exec <automation-path> --input @input.json
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

Semantic validation failure exits nonzero with `error.code: "validation_failed"`
and the report in `error.details` (`error.details.diagnostics` contains the
diagnostics). Do not expect a successful stdout response with `ok: false`.
CLI syntax errors use `invalid_arguments` with a discovery hint; these are
distinct from blueprint validation failures.

`validate`, `parse`, `normalize`, `runs`, `run-show`, and `replay` are
disk-backed. Normalization prints by default; `--write` overwrites the file.
Replay reconstructs state without executing the automation. Live `exec`
requires the desktop app for the same `WARDIAN_HOME` and a blueprint in its
Library. The mock executor is for engine fixtures, not normal execution.

Schedule commands write disk state without the app. `schedule run-now` sets a
due timestamp and unpauses; success does not prove a run launched. The running
app executes due schedules. Inspect run evidence to establish execution.

Bundled, editable examples live in `<WARDIAN_HOME>/library/automations/samples/`.
They are templates only: inspect and adapt one before running it, and create a
schedule only after the user explicitly asks for one.

Schedules require an existing workspace directory. `schedule update`
edits the persisted record in place, so its id and run history remain stable.
Weekly schedules default to a one-week recurrence; use
`--repeat-every <positive-integer>` for a weekly interval from 1 through 520
weeks. The
interval-only `--every` option remains expressed in minutes, and the current
schedule model does not support repeat intervals for other calendar cadences.

## JSON Inputs

Where supported, `--input` and `--assignments` each accept a JSON object inline,
`@file` to read it, or `-` for stdin. A leading UTF-8 BOM is handled. Prefer files
for substantial JSON and shell-independent quoting. Use stdin for only one
argument per command. For non-ASCII input in PowerShell, prefer an explicitly
UTF-8 file: older PowerShell versions may encode native pipes differently.
Discover the object's runtime requirements in the
blueprint or relevant schedule contract; Clap discovery describes syntax only.

```bash
wardian automation exec <automation-path> --input @input.json
printf '%s' '{"target":"HEAD"}' | wardian automation exec <automation-path> --input -
wardian automation schedule add --blueprint <id> --name <name> \
  --workspace <existing-directory> --every 60 --input @input.json --assignments @assignments.json
```

PowerShell:

```powershell
wardian automation exec <automation-path> --input '@input.json'
'{"target":"HEAD"}' | wardian automation exec <automation-path> --input -
wardian automation schedule add --blueprint <id> --name <name> `
  --workspace <existing-directory> --every 60 --input '@input.json' --assignments '@assignments.json'
```

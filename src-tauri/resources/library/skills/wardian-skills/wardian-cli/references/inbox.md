# Inbox

Wardian exposes Inbox as a read/write agent surface.

## Read

Read the newest shared events with:

```bash
wardian inbox list
wardian inbox list --unread --type action_needed,approval_request \
  --source provider_runtime,interaction_store
wardian inbox list --limit 100 --offset 100
```

PowerShell:

```powershell
wardian inbox list
wardian inbox list --unread --type action_needed,approval_request `
  --source provider_runtime,interaction_store
wardian inbox list --limit 100 --offset 100
```

The command returns schema-versioned JSON sorted newest first. Supported type
filters are `action_needed`, `agent_update`, `agent_completed`,
`workflow_completed`, `workflow_failed`, and `approval_request`. Evidence sources identify the
projection: common values are `provider_runtime`, `interaction_store`, and
`live_runtime`. `--unread` excludes acknowledged items. `--limit` and
`--offset` bound a polling page. Reading does not acknowledge, dismiss, or
resolve an event.

The CLI asks the running Wardian app for its assembled projection when the app
uses the same `WARDIAN_HOME`. Without the app, it reads persisted queue items,
durable `notify` records, and workflow-run checkpoints for awaiting approvals
and terminal outcomes where available. Each source is read through a bounded
200-item page; `truncated: true` and `next_offset` identify older source data.
Legacy queue items older than seven days are excluded, matching desktop Inbox
hydration. `--limit` cannot exceed 200.

## Write

Use the write path for information the user needs to act on:

```bash
wardian notify update "The migration passed; one compatibility risk remains" \
  --title "Migration result"

wardian notify approval "Deploy the release" \
  --title "Deploy production" \
  --action "Run the production deployment" \
  --risk "This changes live traffic and may require rollback" \
  --choice "Deploy" \
  --choice "Do not deploy" \
  --wait
```

PowerShell:

```powershell
wardian notify update "The migration passed; one compatibility risk remains" --title "Migration result"
wardian notify approval "Deploy the release" --title "Deploy production" --action "Run the production deployment" --risk "This changes live traffic and may require rollback" --choice "Deploy" --choice "Do not deploy" --wait
```

Prefer `notify update` for a concise material result, limitation, or change to
the user's next decision. Prefer `notify approval` only for irreversible,
external, security-sensitive, or materially costly actions. Keep routine
progress in the transcript. Writes require a managed agent session and the
running app for the same `WARDIAN_HOME`.

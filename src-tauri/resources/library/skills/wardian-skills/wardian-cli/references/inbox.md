# Inbox

Wardian exposes Inbox as a read/write agent surface.

## Read

Read the newest shared events with:

```bash
wardian inbox list --unread --limit 20
wardian inbox list --unread --type action_needed,approval_request \
  --source provider_runtime,interaction_store
wardian inbox list --limit 100 --offset 100
```

PowerShell:

```powershell
wardian inbox list --unread --limit 20
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
uses the same `WARDIAN_HOME`. Top-level `status_source` is `live` for that
response or `persisted` for disk fallback; it differs from item evidence sources
selected by `--source`. Only an unavailable endpoint permits fallback. Live
rejections, protocol errors, and timeouts remain errors.

Without the app, the CLI reads persisted queue items,
durable `notify` records, and workflow-run checkpoints for awaiting approvals
and terminal outcomes where available. The returned page is bounded by
`--limit`; follow `next_offset` when `truncated` indicates more data.
Legacy queue items older than seven days are excluded, matching desktop Inbox
hydration. `--limit` must be between 1 and 200; invalid limits or offsets return
`invalid_limit` or `invalid_offset` rather than silently changing the request.

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

`notify approval --wait` returns the decision or expiry; expiry does not
authorize the action. User Inbox approval and provider permission prompts are
different contracts. Answer the latter only through the explicit approval
action described in [messaging](messaging.md).

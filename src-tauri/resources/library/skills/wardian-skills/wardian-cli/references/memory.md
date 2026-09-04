# Agent memory

Use `wardian memory` for durable, agent-owned continuity. Direct retention and
startup recall work without enabling the optional curator automation.

All memory verbs require a managed session and its valid inherited
`WARDIAN_MEMORY_CAPABILITY`. `--agent` does not grant access to another agent,
and setting `WARDIAN_SESSION_ID` alone is insufficient. The desktop app need
not be running. Store initialization and pending replacement recovery can
write during inspection; see [runtime debugging](runtime-debugging.md).

Save only clear preferences, decisions, corrections, lessons, current project
state, and explicit requests to remember. Default to workspace scope. Use
`--scope agent` only for cross-project preferences or working conventions.
Always include the shortest durable evidence excerpt. Optional `--source`
locators support deep inspection but do not control retention.

```bash
wardian memory save "Prefer compact technical answers" \
  --evidence "The user asked for concise technical answers." \
  --scope agent

wardian memory save "The release checklist is awaiting macOS acceptance" \
  --evidence "The latest review left native macOS validation pending." \
  --kind current

wardian memory list
wardian memory recall
wardian memory show <memory-id>
wardian memory history <memory-id>
wardian memory update <memory-id> "Updated normalized memory" --evidence "Newer evidence"
wardian memory remove <memory-id>
```

Use a known full ID or unique prefix for show, history, update, and remove.
Prefixes resolve within the authenticated agent's ownership boundary. Save and
update accept `--idempotency-key` for retries of the same operation. Update or
remove superseded memories instead of keeping contradictory active records;
remove retains audit history. List/recall return sets, not a token-bounded brief.

PowerShell uses the same command arguments; use backticks only when splitting a
command across lines.

Never say `Memory saved` until the command returns success. Do not treat raw
conversation logging or provider-native session history as memory.

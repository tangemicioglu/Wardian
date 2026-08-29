# Agent memory

Use `wardian memory` for durable, agent-owned continuity. Direct retention and
startup recall work without enabling the optional curator automation.

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
wardian memory update <memory-id> "Updated normalized memory" --evidence "Newer evidence"
wardian memory remove <memory-id>
```

PowerShell uses the same command arguments; use backticks only when splitting a
command across lines.

Never say `Memory saved` until the command returns success. Do not treat raw
conversation logging or provider-native session history as memory.

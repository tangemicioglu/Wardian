---
schema: 2
id: memory-consolidation
name: Memory Consolidation
nodes:
  - id: trigger
    type: manual_trigger
  - id: consolidate
    type: task
    fields:
      agent: role:curator
      prompt: >
        Source agent: {{trigger.output.agent_id}}. Workspace: {{trigger.output.workspace}}.
        Conversation: {{trigger.output.conversation_id}} through sequence
        {{trigger.output.source_sequence}}. Archive available: {{trigger.output.archive_available}}.
        Read only that bounded Wardian conversation archive range. Extract clear durable preferences, decisions,
        corrections, lessons, explicit remember requests, and current project
        state for the source agent. Later evidence supersedes earlier evidence.
        Do not infer uncertain memories and do not quote sensitive content
        beyond the shortest durable evidence excerpt. Return a MemoryCommitBatch
        JSON object. Use idempotency key {{trigger.output.idempotency_key}} and cursor key
        memory-consolidation exactly. Wardian derives the authoritative cursor namespace
        from the authenticated agent, normalized workspace, and conversation ID.
        When archive_available is true, set the cursor conversation_id and sequence to the
        supplied values. When it is false, omit the cursor.
        An empty operations array is a valid result.
      output_schema: >
        {"type":"object","required":["agent_id","idempotency_key","operations"],"properties":{"agent_id":{"type":"string"},"workspace":{"type":["string","null"]},"idempotency_key":{"type":"string"},"operations":{"type":"array"},"cursor":{"type":["object","null"]}},"additionalProperties":false}
  - id: commit
    type: memory_commit
    fields:
      source_node: consolidate
      agent_id: "{{trigger.output.agent_id}}"
  - id: notify
    type: notify
    fields:
      message: Memory consolidation completed. Inspect this workflow run for committed revisions and provenance.
edges:
  - from: trigger
    to: consolidate
  - from: consolidate
    to: commit
  - from: commit
    to: notify
---

# Memory Consolidation

This sample is inert until you run it or bind it to an ordinary workflow
invoker. Choose the curator provider, model, effort, and source agent explicitly;
the selected provider consumes the user's quota and Wardian does not substitute
a fallback. Conversation logging is an optional consolidation input, not the
memory authority. Direct retention and startup recall continue to work when this
workflow is disabled.

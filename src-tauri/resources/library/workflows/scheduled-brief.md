---
schema: 2
id: scheduled-brief
name: Scheduled Brief
nodes:
  - id: trigger
    type: manual_trigger
    fields:
      input_schema: '{"type":"object","properties":{"focus":{"type":"string"},"source_context":{"type":"string"}},"required":["focus","source_context"]}'
  - id: collect
    type: task
    fields:
      agent: role:analyst
      prompt: >
        Collect the relevant facts from the provided source context for
        {{trigger.output.focus}}. Separate confirmed facts, open questions, and
        suggested follow-up work.
  - id: draft
    type: task
    fields:
      agent: role:editor
      prompt: >
        Turn the collected material into a concise operational brief with a
        summary, priorities, and recommended next actions.
  - id: notify
    type: notify
    fields:
      message: A scheduled brief is ready for operator review in the workflow run.
edges:
  - from: trigger
    to: collect
  - from: collect
    to: draft
  - from: draft
    to: notify
---

# Scheduled Brief

Use this template for a recurring summary such as a daily task brief, status
digest, or project pulse. Create its schedule in Wardian only after choosing a
cadence, workspace, source context, and destination. This genericizes the
collect-then-publish shape of a local daily workflow without including any
local source, channel, or recipient.

---
schema: 2
id: research-brief
name: Research Brief
nodes:
  - id: trigger
    type: manual_trigger
    fields:
      input_schema: '{"type":"object","properties":{"question":{"type":"string"},"source_constraints":{"type":"string"}},"required":["question"]}'
  - id: research
    type: task
    fields:
      agent: role:researcher
      prompt: >
        Research {{trigger.output.question}}. Follow any source constraints,
        distinguish evidence from inference, and return links for important
        claims.
  - id: synthesize
    type: task
    fields:
      agent: role:analyst
      prompt: >
        Synthesize the research into options, tradeoffs, risks, and a
        recommendation. Keep citations with the claims they support.
  - id: approval
    type: approval
    fields:
      prompt: Review the research brief before it is shared beyond this automation run.
  - id: deliver
    type: task
    fields:
      agent: role:editor
      prompt: >
        Produce the approved brief in the requested format. Do not publish or
        send it externally unless the operator has separately authorized that action.
edges:
  - from: trigger
    to: research
  - from: research
    to: synthesize
  - from: synthesize
    to: approval
  - from: approval
    to: deliver
---

# Research Brief

Use this template when a question needs evidence gathering, synthesis, and a
human review before delivery. It reflects the research, specialist
coordination, and outcome-review patterns in Anthropic's public agent
cookbooks.

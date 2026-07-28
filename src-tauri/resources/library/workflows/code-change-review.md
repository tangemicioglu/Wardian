---
schema: 2
id: code-change-review
name: Code Change Review
nodes:
  - id: trigger
    type: manual_trigger
    fields:
      input_schema: '{"type":"object","properties":{"change_summary":{"type":"string"},"validation_command":{"type":"string"},"max_cycles":{"type":"integer","minimum":1}},"required":["change_summary","validation_command","max_cycles"]}'
  - id: plan
    type: task
    fields:
      agent: role:planner
      prompt: >
        Create a focused implementation plan for {{trigger.output.change_summary}}.
        Identify the smallest safe change and the checks needed to prove it.
  - id: implement-loop
    type: loop
    fields:
      max_iterations: "{{trigger.output.max_cycles}}"
  - id: implement
    type: task
    parent: implement-loop
    fields:
      agent: role:coder
      prompt: >
        Implement the next safe part of the plan. Keep the change scoped and
        record what remains for the next iteration.
  - id: validate
    type: shell
    parent: implement-loop
    fields:
      command: "{{trigger.output.validation_command}}"
  - id: review
    type: task
    fields:
      agent: role:reviewer
      prompt: >
        Review the completed change and validation results. Report concrete
        risks, missing checks, and whether the change is ready for approval.
  - id: approval
    type: approval
    fields:
      prompt: Review the proposed code change and validation evidence before handoff.
  - id: handoff
    type: task
    fields:
      agent: role:publisher
      prompt: >
        Prepare a concise handoff summary. Do not create external changes unless
        the operator has separately authorized them.
edges:
  - from: trigger
    to: plan
  - from: plan
    to: implement-loop
  - from: implement-loop
    to: implement
    from_port: body
  - from: implement
    to: validate
  - from: implement-loop
    to: review
    from_port: done
  - from: review
    to: approval
  - from: approval
    to: handoff
---

# Code Change Review

Use this template for a bounded code change that needs planning, repeated
implementation and validation, an independent review, and a human approval
gate before handoff. It is inspired by a genericized local review workflow and
the plan-to-validation-to-approval structure demonstrated by Archon.

---
schema: 2
id: conversation-pattern-review
name: Conversation Pattern Review
nodes:
  - id: trigger
    type: manual_trigger
  - id: review-conversations
    type: task
    fields:
      agent: role:evolver
      prompt: >
        Review a bounded, representative set of recent Wardian conversation
        archives. Use Wardian's conversation list and show commands rather
        than recursively reading agent directories. Do not quote sensitive
        conversation content in the report. Identify repeated failure modes,
        recurring friction, and routines that may be worth automating. For
        each candidate, report the observed pattern, likely impact, the
        smallest durable improvement, the best home for it (automation, skill,
        prompt, test, or guide), and whether it needs human approval. Report
        recommendations only: do not modify agents, skills, automations, or
        external systems.
  - id: notify
    type: notify
    fields:
      message: Conversation pattern review is ready for operator review in this automation run.
edges:
  - from: trigger
    to: review-conversations
  - from: review-conversations
    to: notify
---

# Conversation Pattern Review

Use this template to give an Evolver a recurring, bounded review of durable
Wardian conversation archives. It looks for common failures and repeated
manual routines, then leaves recommendations in the automation run for a person
to review. Conversation logging must be enabled for the source agents whose
history should be available. Create a schedule only after choosing the
Evolver, scope, cadence, and review boundary.

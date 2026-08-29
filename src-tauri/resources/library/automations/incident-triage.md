---
schema: 2
id: incident-triage
name: Incident Triage
nodes:
  - id: trigger
    type: manual_trigger
    fields:
      input_schema: '{"type":"object","properties":{"alert_summary":{"type":"string"},"read_only_context":{"type":"string"}},"required":["alert_summary"]}'
  - id: triage
    type: task
    fields:
      agent: role:investigator
      prompt: >
        Investigate {{trigger.output.alert_summary}} using only the supplied
        read-only context. Identify impact, likely causes, confidence, and the
        smallest safe remediation options.
  - id: approval
    type: approval
    fields:
      prompt: Approve a specific remediation plan before any write or recovery action.
  - id: remediate
    type: task
    fields:
      agent: role:operator
      prompt: >
        Apply only the approved remediation, verify the outcome, and document
        what changed. Stop and report if the approved scope is insufficient.
  - id: notify
    type: notify
    fields:
      message: Incident triage has completed. Review the run record and remediation evidence.
edges:
  - from: trigger
    to: triage
  - from: triage
    to: approval
  - from: approval
    to: remediate
  - from: remediate
    to: notify
---

# Incident Triage

Use this template for a human-supervised response to an operational alert. It
keeps investigation read-only until a person approves a specific remediation,
then requires verification and documentation. It is inspired by Anthropic's
public SRE and human-in-the-loop cookbook patterns.

# Workflow Samples

Wardian ships five editable workflow templates under
`<WARDIAN_HOME>/library/workflows/samples/`. They are copied only when missing,
so an edit to a sample remains yours. Samples never run or create schedules on
their own.

| Template | Use it for | Workflow shape |
| --- | --- | --- |
| `code-change-review.md` | A scoped implementation that needs evidence before handoff. | Plan, bounded implementation and validation loop, review, approval, handoff. |
| `scheduled-brief.md` | A recurring status, task, or project brief. | Collect, synthesize, notify; attach a schedule separately. |
| `research-brief.md` | A recommendation that needs evidence and a review boundary. | Research, synthesis, approval, delivery. |
| `incident-triage.md` | A human-supervised operational response. | Read-only triage, approval, scoped remediation, notification. |
| `conversation-pattern-review.md` | An Evolver's recurring review of durable conversation archives. | Bounded archive review, recommendations, operator notification. |

Start by copying or editing the closest sample, then choose the actual agents,
workspace, inputs, commands, destinations, and approval boundary for the work.
For a recurring process, create a schedule only after those details and the
cadence are explicit.

## Privacy and Safety

The samples are genericized from local workflow structures such as review,
daily-brief, and passive-cadence flows. They contain no user names, filesystem
paths, project identifiers, credentials, webhooks, channels, or recipients.
They also use role references instead of real agents and require a human gate
before a code handoff or remediation.

## Public Design Sources

The blueprints are original Wardian examples, informed by public patterns rather
than copied workflows:

- [Archon](https://github.com/coleam00/Archon) demonstrates a coding flow that
  combines planning, implementation, deterministic validation, review, and
  approval.
- [Anthropic Managed Agents cookbooks](https://github.com/anthropics/anthropic-cookbook/tree/main/managed_agents)
  demonstrate iterative test repair, issue-to-PR orchestration, specialist
  coordination, outcome grading, and human-in-the-loop gates.
- [Anthropic Claude Agent SDK cookbook](https://github.com/anthropics/anthropic-cookbook/tree/main/claude_agent_sdk)
  demonstrates research, specialist coordination, observability, and
  human-supervised SRE patterns.

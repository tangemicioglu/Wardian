# Automation Samples

When a user asks to author an automation, inspect the editable samples in
`<WARDIAN_HOME>/library/automations/samples/` before proposing a new graph.

| User goal | Start from | Pattern |
| --- | --- | --- |
| Make a code change safely | `code-change-review.md` | Plan, bounded implementation and validation loop, independent review, approval, handoff. |
| Publish a recurring update | `scheduled-brief.md` | Collect, synthesize, and notify; create the schedule separately after the user chooses a cadence. |
| Turn a question into a cited recommendation | `research-brief.md` | Research, synthesis, approval, and delivery. |
| Respond to an operational alert | `incident-triage.md` | Read-only triage, approval, scoped remediation, and a notification. |

The samples use generic role references, input fields, and no external
destinations. Ask the user which agents, workspace, sources, validation
commands, cadence, and approval boundary apply before adapting a sample. Never
copy local user automation content, paths, names, credentials, or destinations
into a new automation.

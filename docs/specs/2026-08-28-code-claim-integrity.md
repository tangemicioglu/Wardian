# Code-claim integrity

Status: direction adopted 2026-08-28. First version owned by Wardian-QA.

## Context

`docs/specs/2026-08-28-debt-gates.md` converted several contributor-checklist
expectations into gates that fail a build. One expectation resisted that
treatment: prose. Comments, docstrings, test names, assertion messages, and
spec documents are the only artifacts in this repository that nothing reads.
A type is checked by the compiler, a test by the runner, a budget by
`check:budgets`. A comment is checked by nobody.

The question raised was whether agents should be blocked from writing comments
at all, on the theory that an agent uses a comment to justify a decision and
the justification then propagates to the next agent as settled fact.

Prior art considered: the `comment-sicko` reviewer in the `pstack` Cursor
plugin, which runs a default-deny pass over a diff against a five-clause
keep-list and reports without editing.

## The trial

A cold agent reviewed every comment line added by branch `chore/debt-gates`.
Cold means a fresh session with no access to the authoring conversation, so it
could not inherit the rationale that produced the comments. It was given the
`comment-sicko` keep-list plus a sixth clause, and instructed to report only.

Clause 6, added for this repository: a comment survives if a tool in the repo
parses it, verified by finding the parser.

| | |
|---|---|
| Added comment lines reviewed | 367 |
| Proposed for deletion | 263 |
| Kept | 104 |

Keeps by clause: machine directive 78, public API contract 11, external
constraint 14, lint pragma 1.

## What the trial actually established

The 263 deletions are taste. The evidence is the five contradictions, each
independently verified against the producing code before any edit, and each
fixed in commit `3ce4146e`.

1. `scripts/native-e2e-targets.mjs` returned `match[1]` from both arms of its
   tier ternary. The validation never ran and an unknown `// @tier` value was
   accepted. The function's own docstring claimed it read the declaration.
2. `src/features/garden/useGardenWorkflows.ts` null-guarded every read of
   `workflow_list_blueprints` and `workflow_list_runs`, with a comment
   asserting `invoke` could resolve to null. Both commands return
   `Result<T, String>` and cannot serialize to null. This is `AGENTS.md`
   prohibition 4, in the branch that wrote prohibition 4.
3. `src/utils/fileDrop.ts` carried two consecutive JSDoc blocks. The first
   documented `formatDroppedPathsForTerminal` from above
   `resolveTerminalShellId`, duplicating the block already on the right
   function.
4. `src/config/ciWorkflow.test.ts` named `nativeE2eTargets.test.ts`. The file
   is `nativeE2eTiers.test.ts`. Wrong on the day it was written.
5. `BlueprintListResult.next_offset` was declared optional. The Rust struct
   has no `skip_serializing_if`, so the key is always present.

Three of these were false when committed. They survived author review, four
gates, and ten green CI checks, because nothing reads a comment.

The generalisation: the useful unit is not a comment but a **claim**, and the
useful check is whether a claim is true of the code it describes. Finding 1 is
a docstring contradicting its own function body. Finding 4 is a comment
contradicting the filesystem. Comments are where claims are densest, not where
they exclusively live.

## Adopted direction

Reframed as code-claim integrity rather than comment deletion or comment-volume
policy.

**Agent tooling.** A cold Reviewer protocol, tentatively `audit-code-claims`,
covering comments, docstrings, test names, assertion messages, specs, and
declared types and contracts. The reviewer is isolated from the author's
conversation and rationale, and reports only. The Coder owns fixes.

Findings are classified as one of: confirmed false, externally or unverifiably
constrained, encodable internal rationale, narration or redundancy, machine
directive or public contract, or taste.

Only independently verified false claims may block. Uncertainty does not
authorize deletion.

Retain from `pstack`: isolation, and adversarial candidate generation. Do not
retain the persona or the default-delete tiebreak unless later evaluation shows
they improve true-positive yield.

**Project QA.** A project-scoped workflow may run the cold review against base
and head and emit an artifact bound to the head SHA. CI may verify artifact
freshness and unresolved confirmed-false findings. CI does not invoke the
model.

**Wardian core.** No new feature required for the first version. Existing fresh
agents and workflow task nodes are sufficient.

## Rejected

**A global `comment_lines` ratchet in `budgets.json`.** Proposed during design
and rejected. Raw comment count rises legitimately for public APIs, machine
directives, and external constraints. The trial makes the failure concrete: a
ratchet would have taxed the 78 machine-directive lines and 11 public-API doc
lines that the keep-list protected, so adding a native E2E test, which requires
a `// @tier` line, would have cost budget. Track the count informationally if
useful, never as a gate.

**A prose prohibition on comments in `AGENTS.md`.** Two upstream issues report
that a mandatory rule of this kind does not survive a few turns of code
generation. Adopting one would repeat the failure the debt-gates spec
diagnosed, which is a rule obeyed in form while the property it names goes
unchecked.

## Known limits

A cold agent verifies claims against code it can read. It cannot verify a claim
about vendor behavior, a runtime timing property, or anything requiring
execution. Those are the comments the external-constraint class exists to
protect, and they remain unverified. This narrows the unverified surface rather
than eliminating it.

The reviewer's own findings are model output and carry the same status as any
other unverified claim. All five contradictions above required independent
checking before any edit, and the reviewer also produced at least one confident
claim about a document count that was only confirmed by counting. A gate that
blocks on unverified reviewer findings replaces unverified prose with
unverified findings. Confirmation by the author is a precondition, not a
courtesy.

## Handoff

Wardian-QA owns the project-scoped verification setup. The trial evidence is
this document. The reviewer prompt used, including the six clauses and the
report format, is reproducible from the classification list above.

Related: `docs/specs/2026-08-28-debt-gates.md`.

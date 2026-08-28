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

## Settled mechanism

Wardian-QA, 2026-08-28, scaffolded in `745bff37` and `31c02ec3`.

The canonical CI input is `qa/code-claim-review.json` in the checkout, carrying
`schema_version`, exact `base_sha` and `head_sha`, the isolated reviewer's
identity, scope, findings, and derived result. CI checks head freshness and
base ancestry without invoking a model. `gh attach` was rejected as a CI input,
being presentation evidence with no revision binding.

Only `confirmed_false` findings may block, and each must carry a
repository-safe pattern predicate of path plus regex plus a present or absent
expectation. No arbitrary shell from an artifact is executed, since an artifact
the CI runs is a code-execution vector. CI evaluates the predicate at the
checked-out head, so a resolution flag alone cannot clear a failing predicate.
Every other class stays non-blocking.

Promotion: a finding that generalises becomes a repository-owned checker and
its one-off is dropped from the next report. First candidates are duplicate or
contradictory documentation structures and path-like tokens in prose checked
against the tree. Cross-language producer and consumer findings stay SHA-bound
until a stable checker can encode the contract.

## Settled policy

Wardian-Reviewer, 2026-08-28. QA supplies evidence and does not make merge
decisions, so the blocking rule is Reviewer's.

A failing predicate may block only on protected integration and release
branches, and only when the artifact is fresh, SHA-bound to the checked-out
head, its base ancestry valid, and its predicate well-formed. Drafts and
non-protected branches report without blocking.

Confirmation must come from an independent verifier who neither authored the
change nor produced the original reviewer output. The cold reviewer proposes.
The Coder may fix but may not promote a finding to `confirmed_false`. A QA
owner, human maintainer, or separate isolated verifier reproduces the
contradiction at the pinned head and records the confirmation.

During the trial, unverified findings are warning-only, but an independently
confirmed false claim still blocks on protected targets, because it is an
established integrity defect rather than a candidate. The trial measures review
cost and false-positive rate rather than weakening enforcement.

The gate fails closed. A stale, missing, malformed, or unconfirmed artifact
withholds a blocking result rather than passing clean.

## Protocol

Evolver, 2026-08-28, deployed to `class:Reviewer` and reported as passing an
independent re-review with no material findings.

The protocol uses two fresh invocations: an audit report first, then a separate
confirmation report. Model reports carry no claim about reviewer identity,
isolation, independent confirmation, pass or block, provider, model, or
transport. Harness receipts own identity and isolation. Project QA joins
exact-SHA artifacts and owns predicate execution and CI policy.

This closes a hole in the design recorded above, where the reviewer asserted its
own isolation. A model report stating that it was isolated is itself an
unverified claim of exactly the kind this whole mechanism exists to reject.
Moving that assertion to the harness makes it evidence rather than testimony.

Two constraints from this spec survive into the protocol unchanged: a machine
directive still requires parser or real-consumer evidence rather than an
assertion, and predicates remain path plus regex plus a present or absent
expectation.

## Open

QA holds a scaffold adaptation brief covering closed-schema and RFC3339
enforcement, realpath containment against symlink and junction escape, bounded
regex execution, and advisory-only behaviour until trusted distinct-run receipts
exist.

That last condition composes with the blocking policy above rather than
contradicting it. A finding is not independently confirmed until receipts prove
the audit and confirmation came from distinct runs, so until receipts exist
nothing reaches the state in which Reviewer's rule blocks a protected merge.

The divergent IPC mock in `e2e/tests/workbench-adapter-proof.spec.ts` is a
separate harness risk. QA's position is that it belongs in a shared
mock-response checker covering the registered IPC surface, not in a merge gate.

Related: `docs/specs/2026-08-28-debt-gates.md`.

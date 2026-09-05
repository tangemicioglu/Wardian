# Pull request delivery

## Scope and authority

For implementation tasks in the Wardian repository, the default deliverable is
a committed, pushed branch and an open, issue-linked pull request. The
repository's AGENTS.md grants standing authorization: do not stop after local
implementation to ask whether to publish. An explicit local-only request
overrides this default. Read-only investigation, explanation, or review does
not authorize implementation or publication.

This policy applies to Wardian only. Keep shared skills and class/global
instructions project-neutral. CLAUDE.md and GEMINI.md already import AGENTS.md;
do not duplicate the policy in those files.

## Publish verified work

1. Inspect the branch, base, and complete diff. Preserve unrelated work and
   keep one issue's changes per PR. Reuse the matching tracking issue, or
   create one when none exists; this is part of the authorized publication.
2. Run [local CI verification](./ci-verification.md). Narrow reruns to the
   affected category when appropriate. Resolve failures or establish an exact
   base reproduction before recording a limitation; never report an unrun
   check as passing.
3. Use Wardian's `autoreview` workflow when it is available to obtain an
   independent local-agent verdict; otherwise obtain that verdict through a
   reviewer agent. Address blocking findings and record the verdict and
   evidence. Review ends at zero blocking findings; track non-blocking
   follow-ups in a linked issue. If a structured reply is unavailable, inspect
   the reviewer's conversation log for the verdict rather than inferring
   approval from idle status.
4. Commit only the intended files with a semantic message, then push the task
   branch. Do not force-push or overwrite another contributor's work.
5. Open a PR or update the existing PR for that branch. Use the repository
   template, link its issue, and include verification and local-review evidence.
   Use a body file for multiline text. Follow
   [screenshot documentation](./screenshot-documentation.md) for UI changes.
6. Verify the published head, issue link, rendered body, mergeability, and
   current CI checks. Return the PR URL and distinguish local validation from
   pending or failed hosted checks. Do not call the PR ready until the
   repository's four readiness conditions hold.

## Review and publication boundaries

Zero-blocker review means review by local agents, not GitHub approval. Never
request reviewers on GitHub: do not use reviewer flags, review-request API
calls, or the GitHub reviewer UI. GitHub's `reviewDecision` is not a substitute
for the local-agent verdict and is not a reason to solicit GitHub reviews.

Do not change branch protection to clear a hosted review requirement. This
policy authorizes publication, not merging, deployment, or unrelated cleanup.
If publication cannot proceed because of credentials or another external
blocker, report that blocker and the completed local evidence explicitly.

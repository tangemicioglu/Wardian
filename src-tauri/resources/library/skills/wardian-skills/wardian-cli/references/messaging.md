# Messaging

## Send A Message

Use `wardian send` for one-way inter-agent communication:

```bash
wardian send "review this patch" --to reviewer-a1
wardian send --stdin --to reviewer-a1
wardian send --file prompt.md --to reviewer-a1
wardian send "status?" --to class:Coder
wardian send "stand down" --to all
wardian send "review this patch" --to reviewer-a1 --wait-until idle --timeout 10m
wardian send "queue this review" --to reviewer-a1 --queue-policy queue-if-busy
wardian send "only submit live" --to reviewer-a1 --queue-policy live-only
wardian send "deliver on the next safe turn" --to reviewer-a1 --queue-policy mailbox-only
```

Targets can be an agent name, UUID, `class:<ClassName>`, or `all`. By default,
`all` and class targets resolve among neighbors, and a bare name resolves among
neighbors before an exact global fallback. UUIDs and exact names cross that soft
boundary. Add `--scope all` only for global broadcast or class resolution in a
real orchestration task. Use `--wait-until` only for a single-agent target.

For substantial text, prefer stdin or a file to protect quoting:

```bash
cat <<'EOF' | wardian send --stdin --to reviewer-a1 --wait-until idle --timeout 10m
Review the changes since origin/main.
Return findings first, then tests run, then any residual risk.
EOF
```

PowerShell:

```powershell
@'
Review the changes since origin/main.
Return findings first, then tests run, then any residual risk.
'@ | wardian send --stdin --to reviewer-a1 --wait-until idle --timeout 10m
```

Normal sends retain the sender attribution when Wardian knows it. Inspect the
returned `delivery[]`; errors contain per-target delivery details including
runtime state, delivery state, input mode, and channel errors.

When `--wait-until` is used, inspect both `delivery[]` and `watch_error`. A
terminal delivery failure is returned with its exact delivery error and means
the requested provider turn did not start. A `watch_timeout` accompanied by a
submitted or accepted delivery means the later watch condition was not
observed; it does not erase the delivery evidence. A
`delivery_submission_timeout` means no submit-start evidence was observed and
must not be treated as provider acceptance. None of these outcomes authorizes
automatic replay.

`--queue-policy queue-if-busy` is the default. It delivers through a live
provider surface when one is safe; for an off or errored target, an ordinary
message runs through that agent's headless provider transport instead. Wardian
leases the target agent while the headless turn runs and reports it as
`Headless` in the roster; a saved provider conversation is resumed when one
exists, while a fresh run does not invent a provider session. The completed
response is available through `wardian agent watch` and the conversation
archive. `--timeout` bounds the headless run (up to 15 minutes); a concurrent
sender that loses the lease race is queued rather than failing. Timeout or
cancellation terminates the provider's complete process tree before its exact
lease acquisition is released, so a shell wrapper cannot leave a hidden turn
running in the target workspace.

Headless delivery and lifecycle changes use the same durable conversation
lease, acquired before their local lifecycle gate. Resume, clear, pause, and
remove stop before changing the agent when an active headless turn already owns
that lease; wait for the turn to finish and try again.

For a single offline target, `send --wait-until idle` completes when the
headless turn records `provider_applied`. The target's returned status remains
`off`: Wardian does not manufacture a live Idle session merely to satisfy a
completion wait.

`live-only` fails instead of falling back. `mailbox-only` is the explicit
deferred-delivery choice: it queues work without launching a provider process.
Provider slash commands sent with `--as-command` remain mailbox-delivered when
the target is off because they require an interactive provider surface. Use the
policy that matches whether the work should run now or wait for a later live
turn.

## Inspect Conversations

Use `conversation` to retrieve durable message history rather than inferring a
peer's state from terminal scrollback:

```bash
wardian conversation list
wardian conversation list --agent reviewer-a1
wardian conversation list --scope all
wardian conversation show <conversation-id>
```

The default `current` scope is the current agent's conversation set. Use an
explicit agent or `--scope all` only when the coordination task needs a wider
history.

Conversation list/show responses include top-level `status_source` (`live` or
`persisted`). Disk fallback occurs only when the endpoint is unavailable; live
rejections, protocol errors, and timeouts are preserved as errors.

Prefer one agent's index before `show`, which returns the full narrative and
has no CLI pagination or turn selector. For bounded detail, read that known
conversation's `manifest.json` and `turns.jsonl` under
`<wardian-home>/agents/<agent-id>/conversations/<conversation-id>/` before
`conversation.jsonl`. Do not recursively crawl agent directories. Direct
`index.jsonl` readers must keep the latest row per `conversation_id` because
the index is append-only upsert history.

## Send Approval Actions

Use an approval action only to answer an outstanding provider approval. It is
distinct from a normal agent message and supplies `accept` or `reject` as the
default body:

```bash
wardian send --to reviewer-a1 --approval accept
wardian send --to reviewer-a1 --approval reject
```

Do not combine `--approval` with `--as-command`.

## Send Provider Commands

Use `--as-command` only when a provider slash command must be the first input
token. It sends the exact body without `From <sender>:` attribution while using
the provider-aware submit path:

```bash
wardian send --as-command "/goal test" --to reviewer-a1
printf '%s' '/status' | wardian send --stdin --as-command --to reviewer-a1
```

PowerShell:

```powershell
"/status" | wardian send --stdin --as-command --to reviewer-a1
```

Command mode accepts exactly one explicit agent name or UUID. It rejects
`all`, `class:<ClassName>`, and `--thread` with `not_supported`. Successful
responses report `input_mode: "command"`. Threading itself is reserved until
it works end-to-end; do not assume `--thread` silently succeeds.

## Ask For A Structured Reply

Use `ask` when the task needs a named peer's accountable result, not just a
message. It captures a pre-send cursor, sends a backend-owned request ID, and
waits for that peer's `reply`:

```bash
cat <<'EOF' | wardian ask reviewer-a1 --stdin --tail 0 --timeout 10m
Review this patch.
EOF
wardian ask reviewer-a1 --file review-request.md --tail 0 --timeout 10m
wardian ask reviewer-a1 --targets reviewer-a2,reviewer-a3 --file review-request.md --tail 0 --timeout 10m
```

The single-target response includes `request_id`, reply status/body, delivery
evidence, watch events, and retained output. `--tail` defaults to 65,536 bytes;
`--tail 0` suppresses transcript/output text without removing the structured
reply or all events/metadata. A reply status is `done`, `blocked`, or `failed`.

`--targets` adds explicit names or UUIDs to the required positional target;
fan-out requires the default `--until reply`. Inspect every `targets[]` outcome
(`completed`, `timed_out`, `delivery_failed`, `cancelled`) and reply status.
An envelope's `ok: true` or exit zero does not mean every task succeeded.
Broadcasts, class selectors, and `--thread` are unsupported.

A single-target structured reply timeout returns `watch_timeout` and leaves
the request open. Fan-out timeout closes outstanding requests with failed
replies, rejecting late replies. Neither proves provider work was cancelled.
Inspect `watch_error` separately from reply and delivery evidence; do not
automatically replay a timed-out ask. Output-marker mode is compatibility
behavior, not a structured reply.

Complete an ask from the target agent terminal using the request ID and reply
instructions in the delivered prompt:

```bash
cat <<'EOF' | wardian reply ask_0123456789abcdef --status done --stdin
Reviewed the patch. No blocking findings.
EOF
wardian reply ask_0123456789abcdef --status blocked --file findings.md
```

Wardian validates sender identity when `WARDIAN_SESSION_ID` is available.
Replies from ordinary terminals remain available to let a human unblock work,
but their caller identity is not authenticated.

## Inspect And Control Native Delivery

Use shallow syntax discovery for these commands, then inspect bounded evidence:

```bash
wardian schema delivery
wardian delivery capabilities reviewer-a1
wardian delivery show <interaction-id> --evidence-limit 10
```

These commands require the running app. `delivery show` reads native broker
records; not every legacy/live-surface message has one. Capability output can
be unnegotiated; check `native_negotiated` before relying on provider support.

When authorized, `delivery withdraw <id>` removes queued work before submission;
`delivery replace <id> --file revised.md --idempotency-key <key>` supersedes
queued work; `delivery cancel <id>` requests cancellation (or cancels queued
work). Cancellation is not rollback of completed effects.

`send` and `ask` accept `--idempotency-key`, `--deadline` or `--expires-in`,
`--expected-generation`, and `--invalidate-premise`. Their enforcement is in
the native delivery path; do not assume live-surface or mailbox routes enforce
them. Idempotency keys identify the same operation, not permission to repeat
work. Deadline expiry bounds queued delivery, while `--timeout` bounds a wait
or headless delivery. Premise invalidation can steer active work and requires
provider capability. A queued response is not provider acceptance.

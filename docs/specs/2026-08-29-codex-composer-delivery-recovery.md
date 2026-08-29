# Codex composer delivery recovery

## Decision

Automated delivery to an interactive Codex terminal must not send Return until
Codex has visibly applied the bracketed-paste payload to its composer. A native
PTY write receipt proves only that ConPTY accepted the bytes. It does not prove
that Codex's event loop committed the paste before the submit key arrived.

Wardian accepts either of these provider-owned observations after the delivery
cursor:

- the complete normalized literal payload in the active composer; or
- Codex's collapsed `[Pasted Content N chars]` marker after the current
  transaction's pre-payload cursor. Codex may render a literal leading segment
  and collapse only the remainder, so `N` is not necessarily the total prompt
  length and is not used as a payload checksum. The delivery lock and cursor
  scope the marker to the current transaction. Marker recognition tolerates
  terminal row wrapping.

If neither observation arrives within 15 seconds, Wardian withholds Return,
records `payload_apply_unconfirmed`, and does not retry automatically. The
existing 750 ms Codex profile delay remains a minimum settle interval, not the
delivery proof.

## Diagnosed failure

Issue #1058 preserved two live Codex sessions. Wardian-Researcher's terminal
ended at `[Pasted Content 6479 chars]`, while its Codex history contained no
matching user turn. Wardian had recorded successful payload and submit-key PTY
writes followed by `provider_turn_start_timeout`. The provider stayed idle.

The prior readiness recognizer accepted any trailing Codex `›` prompt. It
therefore treated a non-empty composer as ready and allowed every later request
to target the same stale draft. Restart appeared ineffective because the same
large request recreated the paste/Return race after each restart. Healthy
controls showed that a generic turn-start timeout can also be transient, so
timeout alone cannot diagnose the persistent state.

Wardian now rejects a current collapsed-paste marker as compose readiness. If a
turn-start timeout leaves that marker active, delivery reports:

- error and delivery phase: `provider_composer_stalled`;
- observed state: `payload_pending_in_composer`; and
- an identity-preserving recovery command.

A timeout without an active pending-paste marker remains
`provider_turn_start_timeout`. Its guidance is reconciliation, not automatic
retry, because a late provider start is still possible.

## Recovery

After installing a build containing this change, clear an already-stalled Codex
composer with:

```bash
wardian agent restart <agent-name-or-uuid>
```

Restart preserves the Wardian UUID, habitat, and provider session history. It
does not replay the failed request. Before issuing a replacement, inspect the
provider history or delivery record and decide explicitly that the original
turn never started. Reuse a caller idempotency key when the calling workflow
supports one.

`wardian agent doctor <agent-name-or-uuid>` reports
`provider_input_state: stalled_composer`, the recovery command, and
`provider_composer_stalled` in `reasons` when the marker is observable. Doctor
uses a 15-second diagnostic control budget so it does not fail at the 500 ms
fast-read threshold.

## Invariants

- Wardian never sends Return based only on a PTY payload-write receipt for an
  accountable Codex delivery.
- Wardian never automatically replays a payload after its bytes may have
  reached the composer.
- A marker in historical scrollback does not block a later empty composer; only
  the active prompt repaint is considered.
- Recovery never deletes or replaces the Wardian agent identity, habitat, or
  session history.
- Human terminal input remains direct and is not delayed by this automated
  delivery gate.

## Verification

Unit and integration coverage must prove:

- a complete literal payload and a transaction-scoped collapsed-paste
  observation release Return;
- a painted literal prefix alone does not release Return;
- absent payload-application evidence fails without writing Return;
- an active collapsed paste is not classified ready;
- a historical marker followed by a fresh prompt is ignored;
- confirmed provider turn start still produces `provider_accepted`; and
- agent doctor uses the diagnostic timeout and reports the stalled state.

Native real-provider acceptance on Windows must deliver a request larger than
the structured-ask inline threshold to a disposable Codex agent, observe
provider turn start, and verify that no payload remains in the composer. The
preserved production specimens are evidence only and must not be mutated by the
test.

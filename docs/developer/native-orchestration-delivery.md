# Native orchestration delivery

Wardian can deliver ordinary agent messages through a provider's structured
session protocol while keeping Wardian identity and delivery policy
authoritative. Use this surface for independently managed agents that are not
currently attached to a live Wardian terminal.

Maintained native transports are Claude persistent stream JSON, Codex
app-server JSON-RPC, Antigravity persistent stream JSON, OpenCode ACP, and Pi
RPC. Gemini is intentionally excluded. These transports require the provider
CLI but no Wardian orchestration plugin; provider plugins and private team
graphs are not part of the routing boundary.

Start an accountable request with a caller-owned idempotency key:

```sh
wardian ask <agent-name-or-uuid> "Review the change" \
  --idempotency-key <stable-request-key> \
  --expires-in 10m
```

PowerShell:

```powershell
wardian ask <agent-name-or-uuid> "Review the change" `
  --idempotency-key <stable-request-key> `
  --expires-in 10m
```

Use `wardian send` for fire-and-forget delivery. Ordinary messages queue until
the provider is idle. `--invalidate-premise` is the only operation permitted to
steer an active turn, and only providers that advertise that capability accept
it.

Inspect or mutate a delivery by its Wardian interaction ID:

```sh
wardian delivery show <interaction-id>
wardian delivery withdraw <interaction-id>
wardian delivery replace <interaction-id> "Corrected message" \
  --idempotency-key <replacement-key>
wardian delivery cancel <interaction-id>
wardian delivery capabilities <agent-name-or-uuid>
```

Withdrawal and replacement succeed only before provider submission.
Cancellation remains `cancel_requested` until provider evidence confirms the
result. Never automatically retry `submitted_unconfirmed`; inspect its evidence
and wait for late reconciliation or make an explicit replacement decision.

Provider and provider-session identifiers in capability or evidence output are
diagnostics. Address all operations with Wardian agent UUIDs or names and
Wardian interaction IDs.

`send --wait-until idle` waits on the exact native interaction until the broker
records provider-confirmed completion. It does not infer completion from the
roster status. A restored agent that has no terminal input channel reuses its
persistent native session, while an attached PTY remains the preferred live
surface.

For deployment validation, use an isolated Wardian home and native packaged
runtime. Create one temporary agent per provider, pause it, then require two
messages to report `native_provider_session`, `completed`, positive turn-start
capability, the expected native transport, and the same diagnostic provider
session binding. A headless fallback is a failed native acceptance even if the
provider eventually returns an answer.

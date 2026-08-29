# Native Provider Transports for Low-Friction Orchestration

- **Status:** Implemented
- **Date:** 2026-08-27
- **Scope:** Claude Code, Codex, Antigravity, OpenCode, Pi

## Intent

Make Wardian agents easy for an orchestrator to coordinate by delivering
messages through each provider's structured session transport whenever one is
available. Wardian remains the source of truth for agent identity, topology,
mailboxes, routing, delivery policy, and acknowledgements.

"Native" means that the provider's own session controller receives the input;
it does not mean that Wardian adopts the provider's private team graph.

## Findings

Manaflow cmux separates three concerns:

1. A terminal control socket for PTY input and screen inspection.
2. Provider hooks for status, permissions, notifications, and session restore.
3. An `agent-chat` runtime with provider adapters.

The `agent-chat` runtime is the most relevant reference. Its Claude adapter
uses a long-lived `claude -p` stream-JSON process. Its Codex adapter uses one
`codex app-server` process with a native thread per session and `turn/steer`
for active-turn input. Antigravity documents an analogous stream-JSON stdin
and stdout protocol, although it does not expose the same documented
mid-turn-steering surface.

The current Wardian `AgentProvider` abstraction is launch/parser-oriented. The
interactive launch path currently strips Claude stream-JSON flags and launches
Antigravity with its interactive prompt mode. Wardian's existing durable
mailbox and live/headless delivery paths remain valuable fallbacks.

## Proposed transport contract

Add a separate optional native-session capability instead of overloading the
provider identity/parser trait:

```text
NativeSessionTransport
  start_or_resume()
  send(message)
  steer(message)       # optional; urgent active-turn input
  interrupt()          # optional
  read_event()
  dispose()
```

The transport must persist the mapping between the Wardian agent UUID and the
provider session/thread/conversation ID. Provider IDs are implementation
details and must never replace Wardian identity.

Messages should carry a Wardian envelope containing sender, recipient,
message ID, conversation ID, reply-to ID, and delivery policy. The provider
payload should identify the content as an external Wardian message while
remaining an ordinary provider-recognized user input.

## Provider plan

### Claude Code

Run a persistent stream-JSON process and write provider-recognized user events
to stdin. Parse stream events and final turn results. Queue ordinary Wardian
messages until the current turn completes. Consider Claude Agent Teams an
optional provider-owned mode, not the cross-provider Wardian transport.

### Codex

Run `codex app-server` through JSON-RPC. Use `thread/start` or
`thread/resume`, `turn/start` for normal messages, and `turn/steer` only for
an explicit urgent policy. Consume `turn/*` and `item/*` notifications and
handle server-initiated approval requests. Start with one app-server process
per Wardian agent to preserve per-agent `CODEX_HOME` isolation; revisit shared
servers only after thread-scoped configuration is proven safe.

### Antigravity

Run the documented persistent stream-JSON mode. Send one `user` event at a
time and wait for its `result` event before admitting the next mailbox item.
Capture and persist `conversation_id`. Add a parser for the current
`init`/`step_update`/`result` event format. Treat active-turn steering as
unsupported until the provider documents a safe control message.

## Delivery semantics

The common state machine should distinguish:

```text
queued → submitted → provider_accepted → turn_started → completed
                                      ↘ failed
```

`provider_accepted` means the provider transport accepted the request. It does
not mean the model understood or acted on it. Duplicate suppression should use
the Wardian message ID; Codex can additionally use
`clientUserMessageId`.

The PTY/mailbox/headless implementation remains the fallback for providers,
offline targets, interactive-only sessions, and native transport failures.

## Questions for Orchestrator

The next design conversation should establish:

- whether it needs fire-and-forget messages, accountable replies, or both;
- whether an active turn may be interrupted or only receive the next queued
  message;
- whether it needs provider-neutral message IDs and delivery receipts;
- whether the orchestrator should call Wardian's CLI/API or receive an
  in-process control channel;
- whether native structured sessions may replace terminal-backed sessions, or
  must remain visually attached to a PTY;
- what minimum status and failure signals are required to make retries safe.

## Initial implementation order

1. Define and test the native-session contract and provider-independent message
   envelope.
2. Implement Claude stream-JSON transport using the existing event parser.
3. Implement Codex app-server transport and approval handling.
4. Implement Antigravity stream-JSON transport and current event parsing.
5. Integrate native transports with the durable mailbox and retain PTY as a
   fallback.
6. Add native-only and fallback-path tests at the appropriate runtime layer.

## Orchestrator consultation

The Orchestrator reviewed live Wardian delivery records and prioritized the
following requirements for low-friction coordination:

- Keep one Wardian API with `ask/reply` for accountable work and `notify` for
  fire-and-forget delivery. Broadcast should be fan-out over that API rather
  than a separate transport primitive.
- Add cancellation by interaction ID, withdrawal/replacement for queued
  messages, and per-interaction deadlines that end in an explicit `expired`
  state. Numeric priorities and general preemption can wait.
- Require positive provider turn-start confirmation and a late reconciler.
  A provider timeout or unconfirmed submit must not trigger an automatic
  retry. Add caller idempotency keys and retain them across the retry window.
- Gate delivery by session generation. A message for a dead generation must
  become explicitly stale instead of silently entering a resumed session.
- Keep queue-until-idle as the default. Reserve active-turn steering for an
  explicit invalidate-premise case; do not make it a general messaging path.
- Address agents by Wardian UUID/name. Expose provider/session details only as
  read-only diagnostics: generation, delivery state/phase, transport, queue
  age/depth, and evidenced provider input readiness.
- Publish transport capabilities and degradation explicitly, including
  steering/cancellation/withdrawal support, acknowledgement granularity,
  payload limits, and execution ceilings. Native operations remain an escape
  hatch, not the protocol surface used by orchestration skills.

The consultation identified delivery confirmation, reconciliation, queue
visibility/expiry, readiness evidence, restart generation semantics, and
structured payloads as higher-value friction reductions than provider-team
graph adoption. Useful later work includes broadcast quorum semantics,
streaming progress, backpressure, deadline propagation, provenance, cost
accounting, and conversation lease arbitration. Avoid automatic retries from
unconfirmed states, exactly-once claims, numeric priority, broad mid-turn
injection, and PTY parity as a constraint on the native contract.

## Architecture handoff to Wardian-Arch

Wardian-Arch confirmed the boundary and took ownership of the implementation
details. It required one architectural refinement: native transports must not
be exposed directly to orchestration callers and must not own queueing or retry
semantics. Add a Wardian-owned native delivery broker/coordinator between the
provider adapters and the interaction/mailbox stores. The broker admits work,
enforces generation and idempotency rules, reconciles uncertain submissions,
and publishes provider-neutral state; adapters only report provider facts and
perform provider-native operations.

The first implementation milestone is therefore a fake-transport broker
contract covering state transitions, positive turn-start evidence, late
reconciliation, stale-generation rejection, idempotency, expiry, withdrawal,
and no automatic replay after uncertain submission. Provider work follows in
Claude stream-json, Codex app-server JSON-RPC, and Antigravity stream-json
adapters, each with deterministic protocol fixtures and native runtime tests.

The handoff identifies the public interaction API, state vocabulary and
turn-start evidence, reconciliation window, generation lifecycle, cancellation
semantics, envelope/size/redaction rules, capability/degradation schema,
visible-PTY versus headless-native behavior, and supported provider versions
as implementation-blocking decisions. Shared Codex servers, broadcast quorum,
streaming progress, richer accounting/provenance, and provider-owned team
graphs remain deferrable.

## Settled implementation contract

Wardian exposes the contract through `send`, structured `ask/reply`, and the
`delivery` inspection and mutation commands. Callers may supply an idempotency
key, an absolute deadline or relative expiry, an expected Wardian generation,
and the explicit `invalidate_premise` operation. Provider session IDs are never
addressing inputs.

The durable native projection uses these phases:

```text
queued -> dispatching -> submitted_unconfirmed -> provider_accepted
                                            \-> turn_started -> completed
```

Cancellation is a request until a provider confirms cancellation. Withdrawal
and replacement are broker operations and apply only before submission.
`submitted_unconfirmed` is intentionally not automatically retryable. Late
provider evidence may reconcile it forward without replaying the payload.

The first process model is one persistent provider controller per Wardian
agent. The actor serializes ordinary turns, keeps queued work bounded, and is
disposed before Wardian starts or resumes an interactive runtime for the same
agent. A future shared Codex app-server remains out of scope.

## Maintained provider matrix

| Provider | Native mode | Positive start evidence | Cancel | Invalidate premise |
|---|---|---|---|---|
| Claude Code | persistent stream JSON | assistant event after user replay acceptance | control interrupt | no |
| Codex | app-server JSON-RPC | `turn/started` | `turn/interrupt` | `turn/steer` with active turn fence |
| Antigravity | persistent stream JSON | completed `user_input` step | no documented operation | no |
| OpenCode | ACP | first matching session update | `session/cancel` | no |
| Pi | RPC | `agent_start` or `turn_start` | `abort` | `steer` |

Unknown event kinds are tolerated for forward compatibility. Invalid JSON-line
framing after submission fails closed as `submitted_unconfirmed`; Wardian does
not replay the request.

No provider plugin is required for the transport boundary. Each maintained
provider already exposes the needed structured process protocol. Existing
provider hooks may enrich diagnostics and permission presentation, but they do
not acknowledge delivery and are not a deployment prerequisite. This decision
must be revisited only if a provider removes an event required for positive
turn-start evidence.

Provider habitat deployment remains provider-specific but Wardian-owned. Codex
app-server uses the addressed agent's projected `CODEX_HOME`; Antigravity's
include projection copies only the bounded instruction and skill surface and
never traverses the habitat workspace link. Claude and Pi retain a
provider-issued UUID even when Wardian pauses the agent before the first
transcript is durable: the first native launch switches from resume to exact
session creation, then subsequent launches resume the resulting transcript.

An idle restored roster entry without a terminal broker is not a live PTY.
For native-capable providers, ordinary messages reuse the persistent native
actor in that state. A real terminal broker still takes precedence for an
attached interactive agent.

## Version and rollout policy

Native capability is enabled only after a runtime probe and bootstrap handshake
succeeds for the installed provider version. Failed negotiation falls back to
the existing one-shot headless path only before any provider payload may have
crossed. After that boundary Wardian reports uncertainty and waits for
reconciliation or an explicit caller decision.

Protocol fixtures cover all maintained non-Gemini providers. Native runtime
acceptance must additionally launch the installed provider executable, prove a
positive turn start, complete a second turn on the same provider session, and
exercise cancellation where advertised. A provider absent from the validation
host remains unaccepted for that host; fixture coverage is not a substitute.

The Windows validation host accepted two exact-response turns through temporary
Wardian Orchestrator agents for Claude Code 2.1.250, Codex CLI 0.150.1,
Antigravity 1.1.22, OpenCode 1.18.25, and Pi 0.84.2. Each run observed positive
turn-start evidence, provider-confirmed completion, and one unchanged provider
session binding across both turns. The app and CLI were built in an isolated
target directory and used an isolated `WARDIAN_HOME`; the production Wardian
runtime was not restarted.

## External grounding

The broker semantics follow established separation between transport
acceptance and work completion: RabbitMQ publisher confirms versus consumer
acknowledgements, NATS JetStream deduplication and redelivery, and Temporal
Signals versus acknowledged Updates. BullMQ queued-job deduplication provides a
useful analogue for broker-owned withdrawal and replacement. None of these
systems promises exactly-once execution after an ambiguous boundary.

Provider wire behavior is grounded in maintained protocol implementations and
documentation:

- [Claude CLI stream-json flags](https://code.claude.com/docs/en/cli-usage) and
  [Claude Agent SDK control client](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/_internal/query.py)
- [Codex app-server protocol](https://github.com/openai/codex/tree/main/codex-rs/app-server)
- [Agent Client Protocol](https://agentclientprotocol.com/protocol/overview)
- [OpenCode ACP implementation](https://github.com/sst/opencode)
- [Pi coding agent RPC documentation](https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent)

Claude permission requests use provider `request_id` values and matching
`control_response` envelopes. An interrupt control acknowledgement proves only
that the CLI handled the control request; Wardian waits for the subsequent
terminal result before projecting `cancelled`. Reported upstream races and
version-specific stdio permission failures make this a negotiated capability,
not an invariant inferred from CLI presence.

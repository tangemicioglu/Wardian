# Topology authority: single writer (stage 1 of 3)

## Context

`wardian graph link/unlink/ignore/unignore` wrote `topology.json` directly
from the CLI process, while `src-tauri/src/commands/topology.rs`'s
`add_topology_edge`/`remove_topology_edge`/etc. wrote the same file
independently from the app. `ControlRequest`
(`crates/wardian-core/src/control.rs`) already carries every other privileged
mutation (`AgentDelete`, `AgentSpawn`, `AgentUpdate`, ...) through the control
plane; topology was the one exception.

Two writers had already diverged twice, both filed against issue #731 (topology
authority) and #1032 (topology reseed bug):

- **Logic (#1032).** `remove_edge_and_suppress_seed_if_team_pair` — which
  makes a deletion durable against a later `seed_team_clique` reseed — existed
  only on the Tauri path. `wardian graph unlink` called plain `remove_edge`, so
  an edge deleted via the CLI came back the next time a team was edited.
- **Authorization (#731).** The CLI's self-serve rule
  (`crates/wardian-cli/src/graph.rs`'s `caller_context_from`/`resolve_pair`)
  is enforced client-side and is fail-open: absence of `WARDIAN_SESSION_ID`
  is read as unrestricted "operator" access, and any process can unset that
  variable in a child shell.

Issue #731's accepted design
([comment](https://github.com/wardian-app/Wardian/issues/731#issuecomment-5459198563))
replaces the original attested-override proposal with a three-stage fix:
route topology through the control plane (stage 1, this change), replace the
fail-open rule with an explicit operator assertion plus team-scoped
coordinator authority (stage 2), and bound self-link resolution to the
caller's team or workspace (stage 3). Stage 1 was scoped to be independently
shippable: it fixes the two-writers problem and the #1032 regression without
yet changing who is authorized to do what, because a coordinator role has
nowhere trustworthy to live until there is one place authorization is decided.

## Decision

**Topology mutations now have exactly one writer: the running app,** reached
through the same control-plane socket every other privileged mutation already
uses.

- `ControlRequest` gains `TopologyLink`, `TopologyUnlink`, `TopologyIgnore`,
  `TopologyUnignore`, each carrying pre-resolved agent UUIDs (`a`, `b`, name
  resolution stays client-side — it's not a security decision) and
  `caller_session_id: Option<String>`, the caller's `WARDIAN_SESSION_ID` or
  `None` outside a managed session.
- `crates/wardian-core/src/topology.rs` gains the pieces every writer now
  shares:
  - `TopologyOperation` (`Link`/`Unlink`/`Ignore`/`Unignore`) and
    `apply_topology_operation`, which applies one operation to a loaded
    `Topology` and always routes `Unlink` through
    `remove_edge_and_suppress_seed_if_team_pair`. This is the #1032 fix: there
    is no second, plainer removal path left to regress to.
  - `authorize_topology_mutation_v1`, which reproduces today's self-serve rule
    exactly (`None` caller ⇒ unrestricted; a caller not in the known-agent set
    ⇒ fail closed; a known caller must be an endpoint) as a single, pure,
    unit-tested function instead of duplicated per writer. The `_v1` name is
    deliberate: this rule is what stage 2 replaces. It is not what stage 2
    adds.
- `src-tauri/src/commands/topology.rs`'s `dispatch_topology_mutation` is the
  one function that authorizes, applies, and audits a mutation. Both the
  app's UI commands (`add_topology_edge` and friends, now implemented via the
  same `apply_topology_operation`) and the control-plane dispatch arms in
  `src-tauri/src/control.rs` end up at this one seam for the CLI/agent path;
  the UI path stays implicitly full-authority, because it is only reachable by
  driving the app's own webview, exactly as before this change.
- An append is attempted for every attempt — allowed, denied, or a no-op — to
  `<WARDIAN_HOME>/topology/audit.jsonl` (`src-tauri/src/topology_audit.rs`,
  the same append-plus-rotate shape as `remote/audit.rs`, kept as a separate
  file because the two domains have already diverged in field shape). A
  record carries `caller` (`"operator"` or `"agent:<uuid>"`), `operation`,
  `a`, `b`, `outcome` (`"applied"`/`"unchanged"`/`"denied"`), and
  `error_code` for a denial. This satisfies #731's audit-event requirement as
  a side effect of centralizing the write, not as separate plumbing. This is
  a best-effort diagnostic log, not a durable guarantee: an append failure is
  logged (`crate::manager::log_debug`) rather than propagated, so it never
  blocks a mutation the caller is otherwise authorized to make — but it also
  means a write failure can leave a gap in the trail rather than surfacing to
  the caller.

### What did not change in this stage

- The self-serve rule's actual behavior (including the fail-open case) is
  unchanged. Closing it is stage 2.
- Self-link resolution (`agent_snapshot()` using `Scope::All`) is unchanged.
  Bounding it is stage 3.
- The UI's authority model (driving the webview is unrestricted) is unchanged.
- Team coordinator designation does not exist yet. "Coordinator" appears only
  in comments marking where stage 2 attaches.

## Compatibility

**Reads are unaffected.** `wardian graph show/neighbors/activity` still read
`topology.json` directly and work with the app closed, exactly as before.

**Mutations now require the app running.** `wardian graph
link/unlink/ignore/unignore` previously worked with the app closed (direct
file write); they now report `app_not_running` (exit 6) if the control socket
is unreachable, the same failure every other privileged mutation
(`agent delete`, `agent spawn`, ...) already has. This is a deliberate,
user-visible behavior change, not an oversight: it is the direct consequence
of there being one writer instead of two, and it is the change issue #731's
design explicitly calls for ("route `handle_mutation` through the app, as
every other mutating command already requires").

**A stale session id now surfaces as `app_not_running` when the app is also
down.** `caller_context_from`'s "is this UUID a known agent" check still runs
client-side first, for immediate feedback without a round trip — a session id
absent from the CLI's own agent snapshot (live-or-DB-fallback) still fails
with `not_found` before any control-plane call. But the *authoritative*
self-serve/stale-session decision now happens server-side against the live
roster, so a session id that looks locally plausible (e.g. present in a stale
DB snapshot) but isn't live, combined with the app being down, now surfaces as
`app_not_running` rather than a specific authorization error. This is a minor,
correctly-conservative behavior change: the caller still cannot proceed, and
the failure still names the real problem (no reachable authority to ask).

**Confidence note requested by the contract:** the fail-open compatibility
claim ("no coordinator designated == today's behavior") is stage 2's claim,
not this stage's, and is not made here. What this stage claims — that the
self-serve rule's *behavior* is unchanged, only its *location* moved — is
verified directly:
`crates/wardian-core/src/topology.rs`'s `authorize_topology_mutation_v1` unit
tests assert the operator/self/foreign/stale cases bit-for-bit against the
CLI's previous `caller_context_from`/`resolve_pair` test cases, and
`src-tauri/src/commands/topology.rs`'s `dispatch_topology_mutation_*` tests
exercise the same cases through the real dispatch function via
`tauri::test::mock_app()`. Confidence: high for the rule itself; the residual
risk is entirely the `app_not_running` behavior change above, which is
intentional and documented, not hidden.

## Testing

- `crates/wardian-core/src/topology.rs`: `apply_topology_operation_*` (each
  operation, self-pair rejection, team-seed convergence) and
  `authorize_topology_mutation_v1_*` (operator, self-serve allow/deny, stale
  caller) as pure unit tests.
- `src-tauri/src/commands/topology.rs`: `dispatch_topology_mutation_*` against
  an in-process `tauri::test::mock_app()` — self-serve allow, foreign-pair
  deny with an audit record, stale-caller fail-closed, operator allow, and
  `dispatch_topology_mutation_unlink_converges_on_team_seed_suppression`, the
  #1032 regression. This is the layer the bug shipped from: the core
  function's own test (`crates/wardian-core/src/topology.rs:1048` in the
  pre-fix tree) already passed, because it tested
  `remove_edge_and_suppress_seed_if_team_pair` directly rather than the
  caller that was failing to call it. Also `mutate_ui_audits_every_operation_as_operator`,
  covering the UI path's audit trail (see "Review findings" below).
- `src-tauri/src/topology_audit.rs`: append, denial with an error code, and
  rotation.
- `crates/wardian-cli/tests/graph_cli.rs`: unchanged local-validation cases
  (unknown target, self-link, stale session, missing second argument outside
  a session — none of these need a running app) plus one
  `*_without_running_app_reports_app_not_running` case per verb, mirroring
  `agent_cli.rs`'s existing `forced_delete_without_app_running_exits_six`.
  Full mutation and authorization behavior moved out of this file because a
  CLI-subprocess test has no running app to connect to.
- `e2e-native/tests/topology-cli-native.test.mjs`: a real-app test exercising
  `wardian graph link/unlink/ignore/unignore` as actual CLI subprocess calls
  against a running native app — the real `ControlRequest` wire format, the
  CLI's socket transport, the real dispatch arms in `src-tauri/src/control.rs`,
  and the real audit log file — plus the #1032 repro shape at this same real
  layer: unlink an in-team pair via the CLI, add a third member to the same
  team (reseeding the whole clique), and confirm the unlinked pair does not
  resurrect. This is the layer none of the tests above can reach, since they
  all call the shared dispatch function directly rather than going over the
  wire.

## Review findings

`Wardian-Reviewer` reviewed this stage before it merged and found two real
gaps, both fixed in the same change:

- **UI mutations were not audited.** `mutate_ui` (the Graph view's write path,
  used by `add_topology_edge` and friends) called `apply_topology_operation`
  directly and never touched the audit log, so the "every attempt is
  audited" claim above was only true for the CLI/agent path. Fixed by routing
  `mutate_ui` through the same `audit_topology_mutation` helper as
  `dispatch_topology_mutation`, with `caller: "operator"` — regression test:
  `mutate_ui_audits_every_operation_as_operator`.
- **Audit append failures were silently swallowed** (`let _ = ...`). An
  unwritable audit log could not block a mutation the caller was otherwise
  authorized to make, but silently dropping the error contradicted the
  audited-attempt claim just as much as never writing the record. Fixed by
  logging the failure via `crate::manager::log_debug`, matching the existing
  `remote/gateway.rs` precedent for the same audit-append-failure shape.

The reviewer's third finding — that the in-process dispatcher tests do not
exercise `ControlRequest` serialization, the CLI's live socket transport, or
the real `control.rs` dispatch arms — is what the new
`e2e-native/tests/topology-cli-native.test.mjs` case above addresses.

## Debt budget

This stage raised two frozen `budgets.json` ceilings:
`src-tauri/src/control.rs` (9274 → 9357) and `crates/wardian-cli/src/main.rs`
(3051 → 3052). Both are `rustfmt`-canonical growth from genuinely new surface
— four new `ControlRequest` match arms plus one dispatch helper in
`control.rs`, one new CLI error-code mapping arm in `main.rs` — not
unformatted or padded code. The bulk of the new logic and all of the new
tests live in files this budget does not track
(`crates/wardian-core/src/topology.rs`, `src-tauri/src/commands/topology.rs`,
`src-tauri/src/topology_audit.rs`) specifically so this growth is as small as
it can be; hand-compacting the `control.rs` match arms further was tried and
reverted because the result did not survive `cargo fmt` (the compacted form
does not round-trip, so it would fail `cargo fmt --all -- --check`). Raised
deliberately, not silently: this note plus the PR body carries the same
reasoning and diff.

## Follow-ups (not in this change)

- **Stage 2 — authorization.** Remove the inferred-operator path (an explicit
  operator assertion replaces "absence of `WARDIAN_SESSION_ID`"), add
  team-scoped coordinator authority, and persist the coordinator designation
  as team state with a CLI surface.
- **Stage 3 — bound self-link.** Restrict self-link resolution to the
  caller's team or workspace instead of the global roster.

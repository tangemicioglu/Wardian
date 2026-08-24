# Clone Team Membership and Topology Relations

- **Status:** Implemented
- **Date:** 2026-08-24
- **Scope:** Backend and CLI/control clone flows

## Problem

Clone registration is backend-owned, but team placement was previously treated
as a separate watchlist repair. That repair found only the first team
containing the source agent and wrote the watchlist without immediately
seeding the communication topology. A clone could therefore appear in the
right roster group while the Graph view and neighbor resolver still had no
team-derived edges for it until a later watchlist save.

## Decision

When a clone is registered:

1. Copy the source agent into every team that contains the source, inserting
   the clone immediately after the source in each team.
2. Remove the clone from any other team before placing it. This keeps the
   existing single-membership cleanup behavior deterministic if a stale clone
   reference exists.
3. Seed team cliques from the resulting persisted team state immediately.
   This adds the clone's topology relations to every copied team and respects
   topology seed suppression tombstones.
4. Emit both `watchlists-updated` and `topology-changed` after the backend
   placement path completes so roster and Graph projections refresh together.
5. Do not copy arbitrary manual edges from the source. Manual topology edges
   represent explicit communication intent, while team membership is the
   relation that clone semantics can safely reproduce.

The watchlist write path is serialized with other watchlist mutations, including
CLI writers through a shared per-home file lock. Index writes use atomic JSON
replacement so readers never observe a truncated document. If the atomic
topology save fails after membership is written, the previous watchlist bytes
are restored and the error is reported; durable membership is not left ahead of
the graph state. For a legacy version-2 topology, suppression migration runs
against the pre-clone teams before the new clone edges are seeded, preserving
old deletions without suppressing the clone's new relations.

If the source is not in a team, clone placement remains a no-op and no
topology relation is invented. Existing manual edges are never removed or
rewritten by cloning.

## Hand-computed behavior

Given `source` in both `team-a = [source, alpha]` and
`team-b = [beta, source]`, with an existing topology containing only the
team-derived edges for the original members:

```text
t0  roster = [source, alpha, beta]
    teams = { team-a: [source, alpha], team-b: [beta, source] }
    topology = { source-alpha, beta-source }

t1  clone registration creates clone and places it after source in global
    agent order; topology has not changed yet.

t2  persisted placement transforms both teams:
    team-a = [source, clone, alpha]
    team-b = [beta, source, clone]
    clone is absent from any unrelated team.

t3  team seeding adds the missing relations:
    source-clone, alpha-clone, beta-clone
    Existing edges remain unchanged and suppressed pairs stay absent.

t4  refresh events cause the Watchlist and Graph to observe the same durable
    membership/topology state. A later read does not need a compensating save.
```

The source-not-in-a-team trace terminates at `t1`: the clone is registered and
ordered normally, but no team membership or topology edge is added.

## Implementation and verification

The placement helper in `src-tauri/src/commands/watchlist.rs` is shared by
desktop and CLI/control clones through the `clone_agent` command. Unit tests
cover first-team placement compatibility, all-team placement, stale clone
cleanup, persisted state, topology edges seeded for every copied team,
topology-save rollback, and legacy suppression migration.

The communication topology remains the source of truth described in
`2026-07-02-communication-topology.md`; this change only closes the clone
write-path gap and does not change resolver, edge deletion, or manual-edge
semantics.

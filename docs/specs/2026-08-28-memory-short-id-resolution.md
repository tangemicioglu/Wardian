# Memory short-ID resolution

## Status

Accepted for implementation on 2026-08-28.

## Problem

Startup memory instructions display an eight-character prefix of each memory's
UUID so providers can refer to a memory compactly. The CLI previously passed
that prefix directly to exact-ID SQL queries. As a result, `show`, `history`,
`update`, and `remove` failed for the ID form Wardian itself displayed. The
managed CLI then rendered the resulting not-found condition through its
anti-enumeration access-denied response, obscuring the actual usability bug.

## Decision

`MemoryStore` resolves memory references before reading or mutating records:

1. A full ID always wins when it is present in the authorized candidate set.
2. A prefix resolves only when it matches exactly one candidate.
3. An ambiguous prefix returns `memory_id_ambiguous` and asks the caller for a
   longer prefix without listing candidate IDs.
4. Candidate IDs are queried inside the actor boundary. Managed agents can
   resolve only their own records; the explicit operator actor can resolve
   records available to the desktop administration path.
5. Resolved IDs are used for every new revision, audit event, and batch result,
   so a shortcut never creates a second logical memory. Batch idempotency hashes
   use the canonical representation, and replay resolution includes historical
   records so an inactive original can still be replayed.
6. Unknown and cross-agent references continue to use the managed CLI's generic
   access-denied response, preserving the existing anti-enumeration contract.

Active-only resolution is used for update, remove, and mutation batches. Read
and history operations resolve across the actor's full record history so a
removed memory remains inspectable by its canonical ID or unique prefix.

## Verification contract

The core store tests cover show, history, update, and remove through an injected
short ID, exact-ID precedence, unique-prefix resolution, and ambiguity. CLI
tests cover the managed self-memory lifecycle, the distinct ambiguity error,
and continued denial of a peer's full and short IDs. Batch tests cover retries
that switch between short and full IDs after removal.

# Antigravity Provider Receipts

- **Status:** Implemented
- **Date:** 2026-08-04

## Problem

The delivery receipt contract requires a provider-originated `turn_started`
event after the terminal submit cursor. Current Antigravity versions persist
interactive turns in the provider-owned SQLite conversation database, while
the live watcher only followed the legacy JSONL transcript. Mailbox and
heartbeat submissions could therefore reach Antigravity but fail Wardian's
receipt timeout.

## Decision

The Antigravity watcher polls the newest `USER_MESSAGE` step (`step_type 14`)
from the verified conversation database. A strictly newer step produces the
same provider `UserQuery` event used by the JSONL watcher. Restored agents are
positioned at their existing newest step, and repeated polls are ignored, so
history is not replayed and one provider turn cannot create duplicate receipts.

SQLite reads remain read-only. Transient database-read failures are retried by
the existing watcher loop; the legacy JSONL path remains available for older
provider formats.

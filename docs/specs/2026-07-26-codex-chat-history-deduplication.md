# Codex Chat History Deduplication

## Problem

Codex records one assistant response in both an `event_msg` `agent_message` and a completed `response_item`. The latter may add an internal `<oai-mem-citation>` block. The visible answer is the same, but a raw-text comparison considered the records distinct, so the Chat view showed duplicates.

Older conversation archives can retain the same pair even after the live parser changes.

## Decision

The provider transcript adapter removes `<oai-mem-citation>` blocks from assistant message text. Chat-event merging also normalizes every message through the visible-text adapter before deduplicating, including replayed archive events.

The merge still requires matching provider, role, normalized visible text, and distinct sources before it collapses a provider pair. Repeated messages from the same source remain separate user-visible events.

## Verification

- Parser test: a Codex completed response with an internal citation exposes only its visible answer.
- Chat merge test: the `event_msg` answer and the citation-bearing completed response collapse to one clean answer.

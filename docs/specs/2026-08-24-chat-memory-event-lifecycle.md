# Chat memory event lifecycle

## Decision

Memory activity displayed in an agent's Chat view belongs to the currently
active conversation. Wardian establishes that conversation boundary at
provider-session launch, before startup memory injection. When Chat loads, it
filters the agent-wide memory event history at that boundary, then merges
eligible memory events with provider, watch, and archived chat events by their
recorded creation time. The runtime boundary remains available when
conversation logging is disabled, because live Chat still displays memory
activity even when durable archive capture is skipped.

Memory events use the same final sequence assignment as every other chat row.
Events without a timestamp remain after timestamped events and retain their
existing sequence order.

## Rationale

Memory storage is intentionally agent-scoped so durable memory survives
provider restarts. Chat is conversation-scoped, so replaying the full memory
history would incorrectly carry events from earlier conversations into a new
session. The active conversation boundary provides the lifecycle gate without
changing durable memory storage or provider behavior.

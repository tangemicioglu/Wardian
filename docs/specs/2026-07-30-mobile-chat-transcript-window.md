# Mobile chat transcript window

## Context

Opening a mobile agent chat fetched its entire durable transcript and then rendered the newest 80 presented rows. On constrained devices, Markdown rendering for that initial window could block interaction for more than one second.

## Decision

The remote chat endpoint returns the newest 40 events by default, plus a cursor for older events. The mobile client requests an older page only when the user selects **Load older transcript**.

Each page preserves chronological order. The response includes `has_older` and `next_before`; clients must treat the cursor as opaque pagination state for the current transcript snapshot.

## Consequences

- Initial payload parsing and Markdown rendering are bounded to the recent conversation window.
- Older history remains available on demand.
- The gateway currently builds the authoritative transcript before selecting the page, so later server-side archive pagination can further reduce backend work without changing the remote API contract.

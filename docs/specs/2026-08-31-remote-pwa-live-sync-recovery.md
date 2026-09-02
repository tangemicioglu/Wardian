# Remote PWA Live-Sync Recovery

## Problem

The remote PWA can receive a provisional agent roster while desktop startup or
provider restoration is still in progress. The status stream then replaces
those `Restoring` entries with live statuses and triggers a refresh of the
shared Inbox projection. If the stream reports a transport error, the client
must recover both projections without requiring a page reload.

The previous client closed the status socket on an error and suppressed the
socket's close callback. It therefore had no reconnect path for runtime socket
errors. A failure while opening the initial stream also had no retry path. In
either case the initial roster and Inbox response remained stale.

## Decision

- Treat an unexpected status-stream error as recoverable. Detach and close the
  failed socket, then reconnect with the existing bounded exponential delay:
  250 ms initially and at most 5 seconds between attempts.
- Apply the same retry policy when the initial status-stream ticket or socket
  cannot be opened. A 401 remains a session-expired state and does not retry.
- Keep intentional disconnects and session-expiry handling non-reconnecting.
- When a replacement stream receives an agent roster, replace the provisional
  roster and refresh the remote Inbox. Existing request-serial ordering keeps
  an older Inbox response from overwriting a newer one.
- Publish the installed runtime's current status after restoration inserts it
  into the live agent map. The gateway also keeps verified per-session status
  observations independently of the global agent lock, so a temporary
  telemetry/provider lock cannot turn the entire roster back into `Restoring`.
- Return the durable queue and notification portion of the remote Inbox within
  the read request. Reconcile filesystem-heavy automation approvals and
  completions in a single-flight background refresh, cache that projection for
  later requests, and continue to deduplicate it against durable queue items.
  Successful Inbox mutations invalidate the cache and generation-check any
  in-flight refresh before it is allowed to publish a replacement projection.

## Verification

The remote store regression tests cover runtime socket-error recovery from a
`Restoring` roster to a live roster plus a new Inbox item, and recovery from an
initial stream-open failure. Backend tests cover roster lock fallback, durable
Inbox projection, runtime-cache merging, and single-flight refresh bounds.
TypeScript checking and linting must pass before the change is published.

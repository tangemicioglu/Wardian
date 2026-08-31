# Remote PWA Startup Recovery

## Problem

The remote PWA has two independent stale-startup failure modes. A client can
remain controlled by the fixed `wardian-remote-app-shell-v1` worker after a
new shell is deployed, and the application can remain in its loading state
while optional Inbox or automation requests wait on a slow gateway. A
navigation or API request that never completes also prevents useful recovery
feedback.

## Decision

- Version the service-worker shell cache from the source revision at build
  time, update an existing registration before `window.load`, remove legacy
  caches during activation, tolerate individual precache failures, and bound
  navigation fallback at five seconds.
- Bound remote JSON requests at fifteen seconds while preserving caller
  cancellation.
- Make only the authenticated agent roster and watchlist required for the
  initial watchlist render. Load Inbox and automation data in the background.
- Serve the web manifest as `application/manifest+json` so the browser can
  recognize the installed PWA.

This preserves the existing remote API and watchlist behavior while ensuring
that stale shell recovery and slow optional data cannot strand the initial
screen indefinitely.

## Rollout and verification

The desktop application must be rebuilt and restarted so the embedded gateway
serves the versioned worker and updated application bundle. A reopened PWA
then discovers the new worker, which deletes the legacy shell cache during
activation. Regression coverage verifies cache replacement, navigation timeout
fallback, request cancellation, early worker discovery, and rendering before
optional Inbox data completes.

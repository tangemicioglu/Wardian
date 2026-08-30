# Remote PWA Bootstrap Resilience

## Problem

The remote PWA rendered `Loading Wardian...` until the roster, watchlists,
automations, and Inbox requests had all completed. Automations and Inbox data
are secondary surfaces, and their gateway handlers can perform filesystem work
while the watchlist is otherwise ready to display. A delayed optional request
therefore made the entire remote shell appear hung.

## Decision

Remote startup is considered ready when the authenticated roster and watchlist
state have loaded. Automation and Inbox data load after the ready state and do
not block the initial watchlist. Remote JSON requests also have a bounded
15-second transport lifetime; a request that never produces a response moves
the shell to its existing unreachable recovery state instead of leaving the
spinner indefinitely.

The service-worker cache recovery remains independent: it repairs stale
app-shell clients before the application bootstrap runs. This change handles
the separate case where the application has booted and its API bootstrap is
the part that is stalled.

## Verification

Unit coverage proves that the watchlist becomes ready while Inbox loading is
still pending and that a never-resolving remote request is aborted. The remote
PWA browser smoke remains the integrated readiness check.

# Remote PWA Pairing Error States

## Context

The remote PWA previously mapped every bootstrap failure that was not an HTTP
401 session response to Desktop unreachable. That made an expired or consumed
pairing offer, and a revoked device, look like a network outage even though the
gateway had responded and provided a recovery-specific error code.

## Decision

The PWA maps gateway error codes to four user-facing recovery states:

- Pairing offer missing, used, expired, invalid, or its pending request no
  longer exists: tell the user to scan a fresh QR code.
- Device missing or revoked during re-authentication: tell the user to pair
  the device again, clear the stale local device identity, and avoid retrying
  it indefinitely.
- HTTP 401 session failures: request re-authentication.
- Other failures: keep Desktop unreachable and provide transport retry
  guidance.

The frontend uses the gateway's machine-readable code field and never exposes
the internal error detail in the primary recovery message. The existing
gateway protocol and session handling remain unchanged.

## Verification

RemoteMobileApp.test.tsx covers stale pairing, revoked devices, expired
sessions, and genuine transport failures. The remote-control guide documents
the recovery action for each state.

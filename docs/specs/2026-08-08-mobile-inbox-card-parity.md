# Mobile Inbox Card Parity

## Context

The remote mobile Inbox already receives the desktop queue projection, but its
cards rendered every summary at full length and did not expose the agent
session associated with an item. This made long updates consume the screen and
forced users to leave Inbox and find the agent elsewhere.

## Decisions

- Match the desktop Inbox summary threshold: long summaries begin as a
  four-line preview and expose Show details / Hide details controls.
- Preserve the remote card's phone-sized layout while adding desktop-equivalent
  unread, status, timestamp, title, and accent treatment.
- Add an Open agent action only when the queue item includes an agent session
  id; activating it uses the existing remote agent-detail navigation path.
- Keep the change in the mobile presentation layer. Queue persistence and
  remote action authorization remain owned by the existing gateway.

## Verification

- `RemoteInboxView.test.tsx` covers collapsed summaries and agent navigation.
- `RemoteMobileApp.test.tsx` and the mobile PWA E2E cover the integrated remote
  route.

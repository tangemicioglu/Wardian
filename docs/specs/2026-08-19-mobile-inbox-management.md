# Mobile Inbox Management and Navigation

## Problem

The mobile Inbox receives the same projected items as the desktop Inbox, but
it currently behaves as a read-only list. A user can expand a long summary and
open an associated agent, but cannot triage the backlog, filter event types,
answer an action-needed prompt, or resolve an approval without switching to
the desktop app.

## Discrepancy analysis

| Capability | Desktop Inbox | Mobile Inbox before this change | Decision |
| --- | --- | --- | --- |
| Readable card metadata and long-summary preview | Full card treatment, four-line preview, expand/collapse | Present after the existing card-parity work | Keep and test as shared behavior |
| Agent navigation | Opens/focuses the related agent surface | Opens the remote agent detail view | Ported; keep the mobile destination |
| Event filtering | Filter dropdown for action, agent, and workflow events | No filter | Ported as a compact local filter |
| Mark one item read | Card click marks non-pending items read | No read interaction | Ported through a durable remote action |
| Mark all read / clear read | Header actions; pending approvals stay actionable | No controls | Ported through durable remote actions |
| Clear one item | Trash control for dismissible items | No control | Ported; pending approvals remain protected |
| Provider action choices | Sends an explicit parsed choice to the agent | No action controls | Ported using the existing remote agent action route |
| Durable approval choices | Resolves notification or workflow approvals | Displays approval data only | Ported through an authenticated Inbox action route |
| Alert preferences | Desktop and sound preferences in desktop Settings | No equivalent mobile alert surface | Not ported; mobile should not duplicate desktop notification policy |
| Progressive backlog rendering | Desktop-specific large-list safeguard | Phone list is naturally bounded by the viewport | Not ported; keep the mobile implementation simple |

## Design

- Keep Inbox projections owned by the gateway and the existing desktop queue
  persistence. The mobile client may filter the projection locally, but it must
  not invent a second Inbox database.
- Add one authenticated, CSRF-protected Inbox action endpoint for read,
  dismiss, clear-read, and approval operations. The server validates item IDs
  against the current projection before changing state.
- Preserve desktop semantics: unresolved workflow and manual approvals cannot
  be marked read or dismissed, durable update acknowledgements remain durable,
  and provider quick responses only appear when the provider exposed numbered
  choices.
- After a successful action, refresh the projection. This avoids stale mobile
  state when the desktop is open at the same time and keeps workflow/notification
  lifecycle changes authoritative on the backend.

## Verification

- Focused React tests cover filtering, read/clear controls, provider choices,
  approval actions, and agent navigation.
- Rust tests cover the persistence rules for legacy items and notification read
  acknowledgements.
- Remote PWA coverage exercises the mobile Inbox controls against mocked remote
  endpoints.

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
- Serialize desktop and remote queue mutations behind one async-aware app
  lock, and replace `queue/items.json` atomically so concurrent triage cannot
  lose an acknowledgement or expose a partial file. Desktop full-snapshot
  saves merge against the last desktop load so a remote dismissal or read
  acknowledgement cannot be overwritten by a stale snapshot. If no desktop
  load baseline exists yet, the latest persisted projection is authoritative;
  the untrusted snapshot must not resurrect an item dismissed remotely.
- Header actions show in-flight state and mutation failures. A successful
  mutation followed by a failed refresh preserves the success and exposes a
  retryable Inbox refresh error instead of reporting the mutation as failed.
- Provider choices treat delivery and Inbox acknowledgement as separate
  operations. Once the provider accepts a choice, the choice controls remain
  disabled; a failed acknowledgement reports that the response was sent and
  offers a retry for Inbox status without sending the provider choice again.
  The server records a pending delivery on the queue item before dispatch,
  waits for the native/provider delivery boundary, then records the sent
  choice and treats a repeat request for the same Inbox item and choice as an
  idempotent success. If recovery cannot establish delivery, the mobile card
  remains disabled and explicitly reports uncertain delivery instead of
  silently retrying a consequential choice.
- Clear-read operates only on the legacy completion projection. Durable
  notification acknowledgements and read **Action needed** prompts remain
  visible as canonical history, so the mobile control is disabled when no
  legacy completion items are clearable.
- A provider-choice item with pending or unresolved delivery cannot be
  dismissed; its recovery marker must survive until delivery is confirmed and
  the Inbox acknowledgement is complete.
- Pending workflow and manual approvals remain unread and actionable until
  their approval choice is resolved; navigation does not implicitly mark them
  read.

## Verification

- Focused React tests cover filtering, read/clear controls, provider choices,
  approval actions, agent navigation, and provider-send acknowledgement
  recovery without duplicate sends, including a remount with a server-recorded
  choice, uncertain delivery recovery, and clear-read affordance state.
- Rust tests cover the persistence rules for legacy items and notification read
  acknowledgements, including concurrent queue mutations, stale desktop
  snapshot merging, baseline-less remote dismissal preservation,
  pending-approval guards, and atomic writes.
- Remote PWA coverage exercises the mobile Inbox controls against mocked remote
  endpoints.

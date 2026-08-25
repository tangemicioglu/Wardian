# Mobile chat menu and text selection

## Problem

The remote mobile chat transcript exposes message actions through an ellipsis
button. On touch layouts the popup was clipped by the desktop action-collapse
wrapper, so tapping the selector appeared to do nothing. The detail view also
handled every edge-originating touch in capture phase. A text-selection drag
starting near the left edge could therefore be interpreted as back navigation,
canceling the selection and leaving the chat.

## Decision

Keep the shared chat action markup and allow its popup container to overflow on
touch layouts. Restrict the edge-back gesture to non-interactive detail chrome;
transcript text, form controls, links, and menu controls opt out of the gesture.
This preserves the intentional edge-back affordance while leaving browser text
selection and chat actions to their native interaction model.

## Verification

- Remote mobile tests open a message action menu and assert its menu item.
- Remote mobile tests drag from the transcript's left edge and assert that the
  selected agent remains open.
- `npm run test -- --run src/features/remote/RemoteMobileApp.test.tsx`

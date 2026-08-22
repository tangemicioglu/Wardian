# Chat composer send-button states

**Status:** Accepted

## Context

The chat composer keeps its action button visible while empty so the control does
not move when an agent starts or finishes processing. In light mode, the disabled
button used the muted card fill, which is nearly white against the light composer
surface and made the control look detached from the rest of the toolbar.

## Decision

Use one consistent action treatment for the composer:

- Empty and unavailable: transparent disabled ghost control with the muted icon
  treatment used by the attach action.
- Text or attachments present: accent-filled send control.
- Agent executing: the same accent-filled control becomes the interrupt action.

The button remains present in every state so the composer layout stays stable.

## Verification

`e2e/tests/workbench-screenshot.spec.ts` checks the rendered empty and populated
background colors and captures both states as feature-specific PR evidence.

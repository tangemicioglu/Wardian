# Mobile chat text selection

## Problem

The remote agent detail view installed a capture-phase edge-back swipe handler
over the entire screen. Even with transcript opt-outs, that app-owned touch
path competed with the browser's native long-press and text-selection gesture.

## Decision

Remove the custom edge-back gesture and its transcript opt-out marker. Mobile
chat content now receives touch events without an application capture handler;
the visible Back control and browser history remain the supported navigation
paths.

## Verification

- The remote mobile unit test verifies transcript touch movement is not
  prevented and the detail view remains open.
- The existing remote mobile E2E suite continues to cover chat rendering,
  actions, terminal interaction, and explicit Back navigation.

# Workbench tab overflow behavior

## Decision

Workbench tab headers use a compact, shrinkable strip. Tabs may reduce their
label width and ellipsize text before the strip needs to overflow, while the
surface icon and close affordance remain available. The tab viewport and its
Dockview scrollbar wrapper must also be allowed to shrink so pane actions stay
inside the header.

When the compact minimum is reached, Dockview's overflow switcher remains the
single stable arrow affordance. Wardian hides Dockview's clipped-tab count:
that count can change by two when closing one visible tab reveals a previously
clipped tab, which makes it a poor representation of tab state. Selecting the
arrow opens the existing Dockview tab list above rendered surface overlays.

## Verification

- A crowded workbench keeps its tab headers inside the pane and visibly
  ellipsizes long labels before overflow.
- The overflow arrow is visible when tabs still cannot fit at the compact
  minimum.
- Selecting the arrow opens the overflow list, and selecting a listed tab
  activates that surface.
- The overflow list's popup layer has a Wardian z-index above surface overlays.
- Browser coverage: `e2e/tests/workbench-navigation.spec.ts` test
  `compresses crowded tabs and opens the overflow tab switcher`.

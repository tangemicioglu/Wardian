# Workbench Equal Tab Sizing

## Decision

Workbench tabs use an equal flex basis when they share a pane. Each tab keeps
the existing minimum and maximum bounds, while its label remains the flexible
content region and ellipsizes as the pane becomes crowded.

## Rationale

Content-driven tab sizing makes long file names consume more of the strip than
other surfaces, even though every tab has the same navigation role. A zero flex
basis makes the available tab-strip space the source of each tab's width, so a
file tab cannot displace neighboring tabs merely because its title is longer.
The minimum still allows the open-tab menu to take over when the pane cannot
fit every tab, and the reserved close-control slot prevents hover from changing
the tab geometry.

## Verification

- The Workbench browser regression measures all crowded tab widths and requires
  them to remain equal within one CSS pixel.
- The same regression verifies label ellipsis, stable close-button geometry, and
  opening a tab through the tab-list menu.

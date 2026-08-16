# Workbench Tab Sizing

## Decision

Workbench tabs use the existing readable standard width before they are
compressed by a crowded pane. The tab close affordance always reserves its
layout width, even while visually hidden. When the strip becomes constrained,
the tab label is the flexible region and uses ellipsis rather than allowing the
close affordance to expand the tab.

## Rationale

The close control is revealed for the active, hovered, and focused tab. If its
width changes from zero to a visible width, the tab contents and neighboring
tabs move at the moment the user targets the control. Reserving that width
keeps the tab strip stable and makes the label the intentional compression
boundary.

The standard and minimum tab widths remain shared by Dockview tabs and the
Workbench header tab projection. This keeps normal file tabs and other
Workbench surfaces aligned while preserving the open-tab overflow menu for
crowded panes.

## Verification

- `npm run lint`
- `npm run test -- src/layout/workbench/WorkbenchTab.test.tsx src/layout/workbench/WorkbenchGroupHeader.test.tsx src/layout/workbench/DockviewLayoutAdapter.test.tsx`
- `npm run build`
- Workbench browser screenshot evidence for a readable tab strip, a visible
  close affordance, and a crowded strip with ellipsized labels.

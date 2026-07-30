# Files Tab Strip and Top Chrome

## Decision

The Dockview tab strip remains outside every Files surface's scroll ownership.
When a narrow Files renderer reveals a focused control or Markdown follows a
fragment, it may adjust only its local Files viewport; it must not call browser
scrolling APIs that can move an ancestor workbench pane.

The center Dockview tab strip uses the same semantic sidebar-primary surface as
the left and right titlebar zones. This preserves one continuous native top
chrome while active tabs continue to use the main workbench background.

## Verification

- Opening an image file keeps its selected workbench tab in the visible header.
- Focusing each image control in narrow panes keeps the workbench tab reachable.
- Following a Markdown fragment scrolls only the Files presentation viewport.
- Resizing after Files selection keeps the selected tab inside the Dockview
  header.

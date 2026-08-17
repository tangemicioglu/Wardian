# Interactive cursor consistency

## Goal

Every user-activated control communicates its affordance with a pointer
cursor. Controls that are disabled or explicitly `aria-disabled` must not
look actionable. Drag-first surfaces continue to use `grab`/`grabbing`, and
terminal/editor regions continue to use their text or default cursor.

## Surface inventory

| Surface | Clickable elements audited |
| --- | --- |
| App chrome | Sidebar rail actions, left/right pane toggles, window controls, workbench surface tabs, tab close actions, tab menus, and new-surface actions |
| Home and dashboard | Surface launch cards, agent/activity cards, filters, sort headers, refresh actions, analytics links, and agent navigation |
| Watchlist and agents | Agent/team rows, sortable headers, team collapse controls, selection actions, agent card mode/maximize/delete actions, and context-menu items |
| Graph and garden | Relationship lenses, layout/reset/inspector controls, inspector actions, node/edge actions, zoom/fit controls, entity cards, and context-menu items |
| Workflows | Mode tabs, library workflow rows and run actions, builder toolbar/node controls, schedule editor controls, selectable run-DAG nodes, run controls, monitor filters, and run/schedule actions |
| Inbox, commands, and changes | Inbox cards, approval/action choices, filters, dismiss/read actions, command inject/copy actions, baseline selectors, and changed-file rows |
| Library and explorer | Section navigation, library rows, toolbar/menu actions, file-tree rows/expanders, file actions, deployment pickers, and detail-pane actions |
| Files and renderers | File tabs/header actions, comparison controls, recovery actions, renderer retry/zoom/navigation controls, and Markdown links |
| Git and source control | Repository actions, branch/history controls, tree/list toggles, changed-file rows, diff actions, and hunk controls |
| Chat and terminal | Chat attachment/copy/retry/approval controls, terminal restart/hide/workspace actions, and provider/session actions |
| Browser, remote, and settings | Browser navigation/reload/close actions, remote navigation/tab/card/action controls, settings actions, pairing/device actions, and update controls |

## Cursor contract

The shared stylesheet applies `cursor: pointer` to enabled semantic controls:
buttons, links, form choice controls, disclosure summaries, and
interactive ARIA roles (`button`, `tab`, `menuitem`, `option`, `treeitem`,
`radio`, `checkbox`, and `switch`). Existing component-specific cursors take
precedence for drag handles and text-editing regions. Disabled controls remain
outside the pointer selector so their existing disabled affordance is retained.
Static labels are not included unless a component gives an enabled choice
control its own explicit interactive cursor.

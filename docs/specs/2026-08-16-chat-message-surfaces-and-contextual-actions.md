# Chat Message Surfaces and Contextual Actions

Status: Implemented (initial slice)

## Context

Wardian's chat surface is an operator timeline for supervising coding agents,
not a consumer messaging inbox. The primary task is to scan agent output,
recognize intervention points, and open detail only when it is useful.

The previous chat pass reduced tool and work-log density, but message rows
still used a conventional rounded bubble for user turns and rendered copy
buttons as persistent bordered controls. Those affordances added visual weight
and reserved attention for a secondary action on every row.

## Decisions

### Message surfaces

- Assistant responses remain full-width readable prose without a surrounding
  card or bubble.
- User prompts use a compact, lightly tinted right-aligned surface with a slim
  accent edge. The edge distinguishes the author's turn without introducing a
  second heavy card language.
- System and exceptional message roles retain their semantic status surfaces.
- Message copy actions overlay the row instead of reserving space in the text
  measure.

### Contextual copy actions

- The primary copy action is a borderless ghost icon revealed on desktop row
  hover or keyboard focus.
- Rows with multiple copy targets reveal an overflow trigger with the row's
  secondary actions when hovered or focused.
- Touch layouts replace the hidden hover action with a 44px overflow trigger;
  core copy behavior therefore does not depend on hover.
- Activity rows expose output and changed-path copying from the same action
  surface. Work-log and turn-change rows use the same pattern.
- Code blocks retain their visible copy action because copying a code block is
  a task-specific primary action, not a generic row action.
- Copy feedback is temporary and changes the accessible label to describe the
  result.

## Interaction contract

1. A normal transcript render shows no persistent copy border or copy button on
   the message surface.
2. Hovering or focusing a row exposes its primary copy action without changing
   the text width.
3. A touch user can open row actions from the overflow trigger and reach every
   copy target without hover.
4. Escape or clicking outside an open row menu closes it.
5. Existing copy payloads remain unchanged, including markdown messages,
   activity output, grouped work logs, changed paths, terminal fallback output,
   and code blocks.

## Responsive behavior

Container-query behavior continues to govern the chat pane itself. Input-mode
adaptation is layered on top:

- Pointer/hover layouts use compact contextual actions.
- Coarse or hoverless input uses 44px overflow targets and reserves only the
  small amount of edge space needed to prevent action overlap.
- Narrow user turns keep their compact right-aligned treatment and may use up
  to 96% of the available width.

## Verification

- Unit tests cover direct copy behavior and menu-based changed-path copying.
- Remote chat tests cover the touch-safe work-log action menu.
- Code-block copy remains covered by the markdown tests.
- Feature-specific screenshots should show a calm transcript at rest and an
  action state only where the interaction is intentionally revealed.

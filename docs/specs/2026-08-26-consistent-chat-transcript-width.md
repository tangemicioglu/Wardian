# Consistent Chat Transcript Width

## Decision

Use one responsive transcript column for chat content on desktop and remote
surfaces. The column is fluid until it reaches `76ch`, then remains centered.

All rendered transcript rows—messages, tool activity, terminal fallbacks,
work logs, and change summaries—must be children of that column. Individual
message bubbles may still use their role-specific inner sizing, but row-level
content must not escape the shared readable width.

## Rationale

Assistant messages already use `76ch` as their readable line-length limit.
Activity rows previously bypassed that limit on the remote surface, producing
full-width work logs and terminal cards beside narrow prose messages. Reusing
the assistant reading width keeps the transcript visually coherent while
remaining full-width on phones and narrow panes.

## Verification

- At a wide viewport, message and activity rows share the same centered column.
- Below the column limit, content uses the available width without horizontal
  overflow.
- Desktop and remote chat use the same transcript-width class.

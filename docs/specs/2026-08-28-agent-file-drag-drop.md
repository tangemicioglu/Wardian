# Agent File Drag and Drop

## Context

The Explorer already exposes the agent workspace, while Chat and Terminal are
the two agent-facing input surfaces. Users should be able to move a file from
the operating system or the Wardian Explorer directly into either surface
without copying path text by hand.

## Behavior

- Explorer file rows are draggable. A drag carries a Wardian-owned custom MIME
  payload containing the absolute path; folders are not draggable in this
  slice.
- Chat accepts native file drops, file URLs, and Wardian Explorer drops as
  attachment chips. Multiple files remain separate attachments and are sent
  with the existing attachment prompt protocol.
- Terminal accepts native file drops and Wardian Explorer drops. It inserts
  shell-escaped paths at the prompt and leaves a trailing space so the user
  can continue editing. Dropping never presses Enter or executes a command.
- Both destinations show an accent drop target while a recognized file drag is
  over them. Unrecognized text drags remain ordinary browser behavior.

## Compatibility boundary

Native OS paths are local filesystem paths. This change does not upload files
to a remote provider or remote workspace; remote transfer remains a separate
capability. Tauri's native drag-drop event supplements browser drag events so
absolute paths from the operating system are available on desktop builds.

## Design notes

The payload uses a single-path MIME value for simple consumers and a JSON
array MIME value for future multi-selection. Path extraction is shared by Chat
and Terminal, while each destination keeps its existing input contract:
attachments for Chat and non-executing PTY text insertion for Terminal.

# Consistent File Opening Preferences

- **Status:** Implemented
- **Date:** 2026-08-09

## Context and Problem Statement

File paths can enter Wardian through Explorer, chat Markdown, agent terminals,
and the bottom user terminal. These entry points previously disagreed: some
opened a Files surface, while others always launched the configured external
editor. Files without a Wardian renderer could also remain on an empty
“Preview unavailable” state even though the operating system knew how to open
them.

## Decision

Use one frontend routing helper for all file-link and Explorer entry points.
The helper classifies files into the broad families Wardian can render today:

- **Text and code**: text, source, configuration, and Markdown files.
- **Images**: common raster image files.
- **PDF**: PDF documents.

Each family has a persisted `wardian` or `external` preference in
`file_open_actions`. A Wardian preference opens a permanent Files surface when
the Workbench navigation service is available. An external preference uses the
configured external editor setting. Unsupported or otherwise unclassified
files always use the system-preferred application; they do not use a custom
editor as a document viewer.

The old `explorer_file_click_action` value is accepted only while loading older
settings documents. A legacy external value migrates once to all three family
preferences, after which the persisted legacy override is discarded. Current
settings and routing use only the family-specific values.

## Unsupported Files Surface Behavior

When the Files descriptor reports `unsupported_content`, the visible
`UnsupportedRenderer` requests the system-preferred viewer once for that
resource revision. It shows a short launch status while the request is in
flight. If the launch fails, the renderer returns to the metadata state with
**Open With** and **Reveal** recovery actions. Other unavailable states, such
as an oversized file, retain the metadata fallback rather than launching
automatically.

## Entry-Point Contract

| Entry point | Supported family | Unsupported or unknown file |
| --- | --- | --- |
| Explorer click/open | Family preference | System-preferred viewer |
| Chat Markdown file link | Family preference | System-preferred viewer |
| Agent terminal file link | Family preference | System-preferred viewer |
| User terminal file link | Family preference | System-preferred viewer |
| Files Markdown link | Existing authorized Files navigation | Existing Files navigation |

The final native launch remains behind `open_in_external_editor`; selecting
the system destination passes `system` explicitly so configured VS Code or
custom-editor settings cannot intercept unsupported content.

## Verification

- Unit coverage checks family classification, Workbench opening, configured
  external launching, unsupported system fallback, and failed-launch recovery.
- Settings coverage checks legacy migration, sparse persistence, and a
  targeted family preference.
- Terminal-link coverage checks common rendered and unsupported extensions are
  recognized before the shared router is invoked.

# Chat Composer and Attachment Pipeline

- **Status:** Implemented (initial native clipboard slice)
- **Date:** 2026-08-16

## Context

The chat composer supports file picking, native filesystem drops, browser file
drops, and file-URI paste when a filesystem path is exposed. Clipboard image
data is different: a screenshot copied from another application may have no
filesystem path at all. The current path-only model therefore ignores a valid
attachment instead of resolving it into the composer.

The native Tauri clipboard plugin already exposes clipboard images as an
`Image` resource. Provider submission already knows how to place an image back
on the clipboard and inject the provider-specific image-paste shortcut. The
composer should connect those two capabilities.

## Design direction

The composer is a **quiet launch strip**. It keeps the prompt primary, makes
attachments visible but compact, and treats pasted images as first-class
evidence rather than as an opaque clipboard side effect.

## Goals

- Capture clipboard images pasted directly into the native chat composer.
- Represent path-backed files and clipboard images in one attachment list.
- Show a compact, removable attachment chip for every captured image.
- Preserve provider-specific image delivery behavior at submit time.
- Keep pasted image data in memory until submission; do not create unnecessary
  staging files.
- Give the user an explicit error when native clipboard image capture fails.
- Keep the composer usable in compact Grid cards and touch-oriented contexts.

## Non-goals

- Uploading binary attachments through the remote web gateway in this slice.
- Converting arbitrary browser `File` bytes into native filesystem paths.
- Replacing provider-specific image paste shortcuts.
- Supporting video, audio, or rich HTML clipboard payloads.

## Attachment model

`ChatAttachment` continues to support path-backed files and gains an optional
native clipboard image payload:

```text
ChatAttachment
- name: string
- path: string              // filesystem path, or empty for clipboard image
- image: Image | undefined  // native Tauri clipboard image resource
```

The attachment is an image when it has an `image` payload or an image filename.
Clipboard image names are generated as `pasted-image-1.png`, incrementing only
when needed to keep chips and copy text unambiguous.

## Capture rules

1. Resolve filesystem paths first for picker, native drop, file drop, and
   file-URI paste.
2. If a paste contains an image item but no path, prevent the browser's default
   insertion and call the native clipboard manager's `readImage()`.
3. Add the returned `Image` resource as a `ChatAttachment` with an empty path.
4. Keep text-only paste behavior unchanged.
5. If image capture fails, keep the prompt intact and show a concise composer
   error; do not create a fake path or silently discard the action.

## Submit rules

- Path-backed image attachments use the existing `Image.fromPath()` flow.
- Clipboard image attachments use their in-memory `Image` resource directly.
- Each image is written to the clipboard immediately before the provider image
  paste shortcut is injected.
- The structured prompt includes every attachment reference. Path-backed files
  use their path; clipboard images use their generated name.
- Removing a chip removes it from both the prompt reference list and image
  staging sequence.

## Composer behavior

- The prompt remains the dominant control.
- Attachment chips sit above the prompt and use a file or image glyph.
- Chips truncate long names, expose the full path as a tooltip when available,
  and provide a keyboard-reachable remove action.
- The attachment button remains available for path-backed files.
- Send remains disabled when there is no prompt and no attachment.
- Narrow/touch layouts keep remove and disclosure controls at a usable target
  size without making the default composer tall.

## Remote boundary

The remote composer currently submits a text action through the remote gateway.
Clipboard image support there requires an explicit authenticated binary upload
or data-channel contract. Remote image capture should be a follow-up spec, not
an implicit base64 expansion of the existing prompt action.

## Validation

- Unit-test clipboard image capture when the paste payload has no filesystem
  path.
- Unit-test direct staging of an in-memory image resource.
- Preserve path picker, native drop, file-URI paste, removal, and submit tests.
- Verify that text-only paste does not call native image capture.
- Capture one compact composer screenshot with a pasted image chip and one
  submit-ready state after the attachment is removed.

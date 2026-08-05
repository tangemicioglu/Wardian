# Chat Composer Model and File Input

## Decision

The Chat composer keeps the high-frequency controls in one compact toolbar. The
model and reasoning-effort selectors sit beside the message input instead of in
a separate settings card, and the restart-only status copy is removed.

Chat file attachments accept native Tauri Explorer drops as well as browser
drop and paste events when the host exposes filesystem paths. Attachments stay
as exact paths until submission so image files can use the provider's native
paste path and text files can be included in the prompt context.

Changing the model persists the agent configuration and immediately sends the
provider's `/model <model>` command to the active session. Persistence and live
application are reported separately when the provider command fails; a failed
live command does not discard the successfully persisted selection.

## Scope and limits

Clipboard data that exposes only file bytes or a filename, without a filesystem
path, is not silently treated as an attachment. The browser cannot grant the
provider access to such data, so the native file picker remains the fallback.

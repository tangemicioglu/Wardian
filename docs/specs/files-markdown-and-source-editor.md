# Files Markdown and source editor presentation

## Decision

Files keeps Markdown as the portable source of truth. Markdown documents open in a rendered, readable document view and retain a Monaco source presentation for exact edits. Wardian does not introduce a rich-text document model or a secondary serialized representation.

## Rendering contract

The renderer supports CommonMark-style Markdown, GFM tables, task lists, strikethrough, and footnotes. Safe raw HTML remains available through the existing sanitized path. Headings receive deterministic fragment targets, local links remain capability-aware, and local images continue to use renderer tickets.

Rendered code blocks display their declared language and provide an explicit copy action. Tables retain semantic table markup inside an accessible horizontal-scroll container so wide tables remain readable without changing their structure.

## Source editor contract

Monaco remains the sole editable buffer. Its model is shared across Files presentations, while each presentation owns its view state. The Files configuration follows Wardian theme tokens, improves source readability with a coding font stack, line rhythm, bracket guidance, sticky scopes, and smooth navigation, and wraps Markdown source while leaving ordinary code horizontally scrollable.

## Reference assessment

Onorca's public documentation describes a rendered-first Markdown experience with a raw Monaco escape hatch. Its editor implementation is not publicly indexed or source-mapped, so Wardian adopts that interaction model only where it preserves Markdown-as-Truth and the existing Files security contracts.

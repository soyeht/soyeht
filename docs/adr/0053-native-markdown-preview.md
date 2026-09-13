# ADR 0053: Render Markdown from the bounded editor viewport

Status: accepted · 2026-09-13

## Context

The editor must show a Markdown preview beside the file. The file protocol
deliberately exposes bounded viewports instead of whole-file payloads. Repository
content is untrusted presentation input and must not gain an external side effect.

## Decision

The Mac client parses only the lines in the current `file.read` viewport with
Foundation's native attributed-string Markdown parser. It never creates a web view,
does not interpret HTML as executable content, and removes link destinations from
the attributed result before display. The preview reports its line range when it is
showing only part of the file.

The shared client owns the bounded preview model and its safety tests. The Mac view
owns only layout and theme. The existing file revision and viewport remain the
single source of content; the preview adds no protocol or persisted state.

## Consequences

Preview work is capped by the same line and byte budgets as the editor wire surface.
Large documents remain incremental as the viewport moves. Links remain visible as
text but cannot open a destination from the preview. Rich features that require a
browser or whole-document processing are outside this slice.

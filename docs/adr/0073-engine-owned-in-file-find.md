# ADR 0073: In-file find and replace run in the engine

- Status: accepted
- Date: 2026-10-01

## Context

The Mac editor holds one bounded page of a file. AppKit's find panel searched
that page only, so it reported wrong counts and could not reach matches outside
it, and its replacements bypassed the engine. The iPad needs the same search, so
the logic cannot live in one AppKit view.

## Decision

`file.find` and `file.replace` operate on the engine's whole buffer, matching
Zed's buffer search. A `FileFindQuery` carries the text and three toggles: case
sensitive, whole word and regular expression. `soyeht-editor` extends the
project-search `ContentMatcher` (`memmem` for exact literals, `regex`
otherwise, case folded through the regex engine) with whole-word filtering,
exact-match checks and `$1` capture expansion.

`file.find` names the client's revision, a starting position and a direction.
In one streaming pass it counts every match, picks the first match at or after
the position (or the last before it), wrapping at the ends, and returns the
matches inside the requested line window (the loaded page, at most 500) for
highlighting. It never stores matches between calls.

`file.replace` takes a base revision and an idempotency key. With a `range` it
replaces that one match, after checking that the range still is a match; without
one it replaces every match. Replace All folds nearby matches into one edit and
sends at most 32 edits, so one buffer change covers any number of matches. Both
go through the durable write path as a standalone undo transaction that never
joins a typing burst, so `file.undo` reverts a Replace All in one step. A
replacement larger than the 16 MiB undo budget is applied but cannot be undone.

The shared client's `EditorFindController` owns the query, options, status
("3 of 41") and commands; `EngineClient.findInEditorFile` pages the view to the
match and selects it. The Mac bar binds to the controller, takes `Cmd+F`,
`Cmd+Option+F`, `Cmd+G`, `Cmd+Shift+G` and Escape, and AppKit's find panel is
off for the editor.

## Consequences

Counts and navigation cover the whole file on every client. A search holds the
engine lock for one pass over the text, which on the 50 MiB editor limit takes
tens of milliseconds for a literal. Typing never triggers a search; the bar
re-counts after a revision change only while it is open.

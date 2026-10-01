# ADR 0071: Engine-owned undo history and one edit path

- Status: accepted
- Date: 2026-10-01

## Context

The editor text view holds only a bounded page of the file, and the engine owns
the buffer. Typing reached the engine through custom insert and delete paths,
but AppKit's own paths did not: paste, cut, drag and drop, find-panel
replacement and input-method composition changed the local page without an
engine edit, and `Cmd+Z` rewound AppKit's local stack, which recorded none of
the custom edits. The page and the engine drifted apart until the next reload
discarded the difference. The iPad and iPhone mirror the same files and need the
same undo, so a stack inside one view cannot serve them.

## Decision

The engine keeps a bounded undo history per open file (`soyeht-editor`
`EditHistory`). Every accepted buffer change prepared by `prepare_edit`
(`file.write`, `file.format`, writing formatting and variants) records one step:
the edits as sent and the edits that restore the replaced text, in UTF-16
positions. Steps closer than 300 ms join one transaction, so a burst of typing
undoes as one unit, as in Zed. The history keeps at most 200 transactions and
16 MiB of edit text, and lives in memory only; it starts empty after an engine
restart.

The history is pinned to the file revision it last recorded. A change it did not
record (disk reload, version revert, project replace, agent review)
moves the revision, so the next undo finds the history stale and drops it
instead of replaying positions into different text. A new edit drops the redo
stack.

`file.undo` and `file.redo` take the client's base revision and an idempotency
key. They replay the newest transaction through the same durable path as
`file.write`: the draft and its author persist before the reply, autosave,
version history and diagnostics follow, and a repeated key returns the stored
result. The result says whether anything was applied and where the cursor
belongs; an empty or stale history answers `applied: false` without changing the
file.

On the Mac, the text view never edits its storage on its own. AppKit's
`shouldChangeText` path becomes a verbatim engine edit or is refused, marked
text stays local until the composition commits as one insertion, AppKit's local
undo is off, and `Cmd+Z`, `Cmd+Shift+Z`, the Edit menu and Vim `u` call the
engine. Edits wait while a replay is in flight until its page is on screen.

## Consequences

Undo and redo behave the same on every paired client and reach text outside the
loaded page. Agent edits on disk are never undone by the human's history; they
end it instead, which loses the older steps but never corrupts the file. Selection
restore is limited to placing a caret after the first restored replacement.

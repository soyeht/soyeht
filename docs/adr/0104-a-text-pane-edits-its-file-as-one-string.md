# ADR 0104: A text pane of the desktop client edits its file as one string

- Status: accepted
- Date: 2026-10-22

## Context

The Mac draws a text (a writing pane of kind `text`, ADR 0061) with TextKit. Every writing pane
is backed by the owned-file path of ADR 0046 and ADR 0047: the engine keeps the file, its draft,
its history and its conflicts, and saves the draft shortly after the last edit
(`limits.editorAutosaveMs`). A text view that edits the page of lines the editor replica holds,
line by line, would repeat everything that the editor pane's keys already do for code (auto
indent, brackets, tab stops), none of it wanted in prose.

The desktop client (ADR 0084) has one text editing model with the rules of a text field, which
the agent composer uses: `soyeht_client::text_input` (a selection, composition, word and cluster
motions, undo and redo, line breaks of any system made one).

## Decision

**The text is a field, the file is the editor's.** A text pane holds its whole document in a
`TextInput`, drawn by the same field the agent composer is (`ComposerField` with prose behavior:
Return adds a line, the Markdown parts are drawn in their look, the pane scrolls to the caret).
The file itself stays in the editor replica (`soyeht_client::editor`), read with `file.read`
like any file and written with `file.write` to the revision the write before it made, so the
queue, the revision chain, the conflict flag and the engine's draft are the editor's, not a copy.

**One edit per change.** What the person typed reaches the replica as the one edit that turns the
page's text into the field's (`writing::document::change_between`: what lies between the text
they share at the start and at the end), in UTF-16 columns as the engine names them. It is made
in the page at once and queued like a keystroke of the editor (`EditorFile::retype`). The page
and the field are the same text whenever the field is drawn; when the engine changes the page (an
agent, another client, a format) and no edit is on its way, the field takes the page's text, and
an edit the engine would refuse for its size is refused in the client and the field shows the
document again.

**Whole documents only.** The field needs the whole document, so a text longer than one page of
`editorViewportLines` lines is not edited, and its pane shows only a notice, not the text. The same
holds for a text with a lone `\r` in a line, which the field would read as a line break and the
page as a character. A scroll model, and a read-only view of the loaded page, are follow-ups.

**Writing methods.** `writing.document.open` and `writing.document.create` open a document in a
pane (the answer carries the pane, the document and the first lines of the file, and a
`pane.created` event carries the same for a pane another client made); `writing.section.list`
lists a section a page at a time (`x-clientPolicy` `pageSize.writingSection`); `writing.structure`
gives the words, asked for the revision the text settled at and never while edits are on their
way; `writing.format` formats a selected range to the revision the document is at, after the edits
before it have arrived, and the page is read again; `writing.changed` makes a replica that is
behind read the page again.

**One companion.** An agent split off a document is its companion; a document that has one is
not given a second (the button focuses the agent), and the button waits while the agent opens.

**Rules both clients share.** The names of the sections, the counts, what the new-document form
submits, which documents a search keeps, the name a pane shows, when closing asks first, the keys
of the formats and the buttons over a selection are `contracts/vectors/writing.json`; the Swift
client passes it through `WritingDisplayText`, `WritingDocumentDraft`, `WritingSectionSearch` and
`WritingChromePolicy`, the desktop through `soyeht_client::writing::rules`. An agent split off a
document is its companion in both clients (`companionSplit` of `pane-layout.json`).

## Consequences

Typing in a text costs one `file.write` per change, as in the editor; a paste is one edit. The
field's own undo walks its own steps and each is one more edit, so the engine's history records
them. A selection is not kept across a format: the engine returns the text with its marks and the
caret goes where the selection ended.

The Mac hides the Markdown marks of the lines the caret is not on; the desktop shows them. Showing
the Mac's view needs a display text of its own (the Mac conceals marks with zero-width characters
of the same length in UTF-16 units, which are three bytes each in a Rust string), and is left for
a later change with a vector of its own.

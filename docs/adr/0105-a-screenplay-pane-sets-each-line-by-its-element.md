# ADR 0105: A screenplay pane of the desktop client sets each line by its element

- Status: accepted
- Date: 2026-10-23

## Context

The Mac draws a screenplay (a writing pane of kind `screenplay`, ADR 0061) as a page: each line
is aligned, set in, bold or in the secondary ink as its element says (scene heading, action,
character, parenthetical, dialogue, transition, shot), the markers that force an element (`.`,
`@`, `>`, a trailing `:`) are kept in the text and not shown, Return and Tab end a line and start
the element that follows, and the person never edits anything but the file. The engine reads the
file (`soyeht-writing`) and says which element each line is in `writing.structure`; it keeps the
file, its draft and its history as for every writing pane (ADR 0046, 0047, 0104).

The desktop client (ADR 0084) edits a text as one string in a field (ADR 0104). A screenplay needs
the same, and lines drawn differently from one another, which the field's one block of text
cannot do: a text block has one alignment, and GPUI places the caret of a centered line as if it
were left aligned.

## Decision

**The same pane, another page.** A screenplay pane is the text pane of ADR 0104 (the same field,
the same `file.write` per change, the same autosave, close question and companion agent) whose
field draws a page. `writing.structure` is asked as it is for a text, and also gives the scenes
(the header counts them, as the Mac's does) and the element of each line.

**Which element a line is.** The engine says it for the text it read. While the person types,
the lines move with the edits (`writing::screenplay::LineKinds`): a line that was not touched
keeps its element wherever it went, a line inserted has none until the engine is asked again, and
a line the person made an element by Return or Tab is that element at once. An answer is taken
for the lines only when the text it read is the text now (the revision it settled at, and nothing
typed since); the scene count is taken always. A line with no element is drawn plain.

**One page, line by line.** The field lays the text out one line of the file at a time, in
Courier Prime at the Mac's size, line height and margins (desktop tokens), each line as its style
says. Alignment is made by the layout (a centered line is a box in the middle of its row), not by
the text's alignment, so that the hit test and the caret are right. The markers of a line are left
out of what is drawn and the caret and the selection are mapped over them, as on the Mac; the text
the engine holds is untouched. The field keeps the questions it asks of a layout (where an offset
is, which offset a point is) behind one type for a block and for a page.

**Return and Tab.** Return ends the line and Tab does the same with the other element the Mac
offers; both make the edit of `ScreenplayInteraction.advanceEdit`: the line stays, a blank line
follows, and the next element starts on the line after it with its mark. Tab on a line with
nothing on it only seeds the element there. Shift+Return adds a line. A screenplay has no
Markdown, so it has no format keys and no toolbar.

**Rules both clients share.** Which element follows, the line an element is set as, the edit
Return and Tab make, and the style and hidden markers of a line are
`contracts/vectors/screenplay.json`. The Swift client passes it through `ScreenplayInteraction`
and `ScreenplayLineStyle`, the desktop through `soyeht_client::writing::screenplay`. The sections,
counts and form are `writing.json` as for texts.

## Consequences

A screenplay is edited under the limits of a text: one page of `editorViewportLines` lines, no lone
`\r`; beyond them its pane shows only a notice.

Left out, to follow: the scene rail and the page numbers, the idea board and the variants of a
line, the SmartType suggestions and the element menu over the line (an element is chosen by
Return and Tab, or by typing its mark), the focus mode, the scaling of the page to the pane, and
the wrapped lines of a centered element drawn centered (they are left aligned in their box).

The Mac's `formattedLine` also trims a full stop at the end of any text it sets as an element; the
vector holds no case for it, and the desktop client has no use for it yet beyond the empty seed
Return and Tab make.

Tab after dialogue seeds `()` and leaves the caret after it, as the Mac does; both clients would
serve the person better with the caret between the parentheses, which needs a caret in the vector.

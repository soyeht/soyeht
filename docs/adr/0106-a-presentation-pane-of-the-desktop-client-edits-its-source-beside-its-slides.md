# ADR 0106: A presentation pane of the desktop client edits its source beside its slides

- Status: accepted
- Date: 2026-10-23

## Context

A presentation is a Markdown file that the engine splits into slides (ADR 0061): `writing.structure`
gives each slide's lines, heading, notes and image paths (`soyeht-writing`, `segment_slides`), and
the person never edits anything but the file. The Mac draws a presentation pane as a rail of the
slides (a thumbnail for each, or a strip of numbers while the pane is narrow), the Markdown source
in a 16:9 stage with its headings, its `---` dividers and its images marked, and a line under it
that says what the source means. A slide show, started from the pane's options menu, presents the
slides one at a time on a letterbox, in a window of its own.

The desktop client edits a text as one string in a field (ADR 0104) and a screenplay as the same
pane with a page (ADR 0105).

## Decision

**The same pane, another body.** A presentation pane is the text pane of ADR 0104 (the same field,
the same `file.write` per change, the same autosave, close question, conflict banner and companion
agent) whose body is the Mac's: the rail, the stage and the footer. The field draws the source in
JetBrains Mono at the Mac's size and line height, with the lines that have a look
(`writing_style::presentation_highlights`); it has no toolbar and no format keys, as on the Mac.

**The slides are the engine's.** The client does not split the source. The slides come with the
word count that `writing.structure` answers for the revision the text settled at, so the rail and
the header's count ("3 slides") follow the engine a moment after the typing, as on the Mac. The
slide the caret is on is the one whose lines hold its line (`slide_at_line`); a click on a slide of
the rail puts the caret on its first line and scrolls the source to it.

**Layout from the size drawn.** The pane is laid out from the size of its body, which a canvas
records on each frame (the terminal pane does the same for its screen): the thumbnails stand beside
the stage when the body is wide enough for the rail, the smallest stage and the margins, else a
strip of numbers stands over it; the stage is the largest 16:9 rectangle that fits the room left,
between the Mac's smallest and largest stage. The measures are desktop tokens, and
`contracts/vectors/presentation.json` holds the Mac's own, so a change of either side fails the
other's tests.

**A slide show over the pane.** The Present button of the header (the Mac has it in the options
menu) puts a slide show over the body, which stays in the window: the letterbox colour of the
palette, the slide on a 16:9 stage with its heading large and centered. The right arrow, Page Down,
Space and a click go to the next slide, the left arrow and Page Up to the one before, Escape
closes it; a key held with the command key is not the show's. The show's position and these rules
are `presentation::Show` and `show_command`, and the sizes of the slide are `slide_metrics`.

**Rules both clients share.** The words of a slide (the title of a slide with no heading, the
line for a slide with no notes), the hint under the source, which slide a line belongs to, the
layout cases and the whole slide show (inputs, position, stage, sizes) are
`contracts/vectors/presentation.json`. The Swift client passes it through `WritingDisplayText`,
`WritingSlideSelection`, `WritingSlideShowPolicy`, `WritingSlideShowState` and
`WritingLayoutPolicy`, the desktop through `soyeht_client::writing::presentation`.

## Consequences

A presentation is edited under the limits of a text: one page of `editorViewportLines` lines, no
lone `\r`; beyond them its pane shows only a notice.

Left out, to follow: the images (the thumbnail marks that a slide has one, the source marks the
line, and neither the rail nor the show draws it: that needs the engine's asset read and a decoded
image view), the speaker notes in the show, a show in a window of its own and full screen, the
mouse pointer that hides while the show is idle, the font and width options of the options menu, the
rail scrolling to the slide the caret is on, and a presentation longer than one page of
`editorViewportLines` lines.

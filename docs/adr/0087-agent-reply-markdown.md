# ADR 0087 · Agent replies are drawn as Markdown, by one set of shared rules

Date: 2026-10-11
Status: accepted.

## Context

Agents write Markdown: bold, inline code, lists, headings, fenced code, quotes, tables and
links. The panes showed it as raw text, with its asterisks and backticks. The Mac client and
the desktop client (Linux and Windows) must draw the same thing, and a reply still arriving
must not jump around as its markers close.

## Decision

### The rules are data in `contracts/`

`contracts/vectors/markdown.json` is the specification. Each vector is a Markdown input, a
`streaming` flag and the blocks it must become: paragraph, heading, code (with its language),
quote, list (bullet or ordered with its first number), table and rule, with runs of text styled
bold, italic, struck, code or linked. Both clients pass the whole file:

- Swift: `MarkdownTests` in `apps/shared` (`MarkdownReply.parse`).
- Rust: `tests/markdown_vectors.rs` in `apps/desktop/crates/soyeht-client`
  (`markdown::parse`).

A rule changes by changing the vector first. The rules beyond CommonMark itself:

- A link is kept only for `http`, `https` and `mailto` addresses. Any other destination (a
  script, a relative path) leaves its text plain.
- A link whose text is its own address is plain text; the pane links bare addresses itself,
  after the parse, with one rule on both clients (`contracts/vectors/web-links.json`,
  `WebLinkRouting.links` and `markdown::link_blocks`): an `http://` or `https://` address up
  to the next space, angle bracket or quote, without the punctuation of the sentence after it
  (`. , ; : ! ? ) ] }`), with a host and no name or password. The desktop never links inside
  code (a code span or block) or over a link the author wrote.
- An image is its alt text, linked to the image when the address is allowed.
- Raw HTML, and a task list's `[ ]` or `[x]`, stay as typed.
- A line break (soft or hard) is a `\n` in the text, in the style around it.
- Empty code blocks and empty list items are dropped.
- Only agent replies and thoughts are drawn as Markdown. What a person or a peer agent typed,
  and system notices, stay plain text.
- The editor's Markdown preview (a Markdown file open in the editor, split view) is drawn by the
  same parse and the same view as an agent reply (`MarkdownReply.parse`, `MarkdownBlocksView`),
  so a document keeps its headings, paragraphs, lists, quotes and code. A file's content is not
  trusted to open anything: the preview's links take the accent and an underline and open
  nothing.

### Existing parsers, not our own

CommonMark parsing is done by swift-markdown 0.9.0 (cmark-gfm, pinned in
`apps/shared/Package.swift`) on Apple platforms and pulldown-cmark 0.13 on the desktop, whose
only features are the parser itself. Foundation's `AttributedString(markdown:)` was tried first
and rejected: it drops bold, italic and code inside link text, which is how agents write file
links. The clients own only the mapping from each parser's tree to the vector's blocks.

### A reply that is still arriving

Parsing is a pure function of the text, so the pane parses the whole reply each time it grows.
While a message is incomplete, the text goes through one more rule first: the spans it has left
open are closed (an open code span, `**` or `~~`), so a bold word shows its style as it is
typed instead of showing raw markers and then jumping. A delimiter that ends the text with
nothing after it yet is dropped, an open code fence is left alone (the parser already runs it
to the end) and a blank line ends every open span. The rule has its own vectors
(`"streaming": true`), implemented in `MarkdownStreaming.swift` and `markdown/streaming.rs`.

Each client keeps the parse of a reply until its text changes (`MarkdownReplyCache`,
`markdown::Cache`), so a transcript that redraws does not parse the replies it already has.

### Drawing

Pane interiors stay flat and palette-only (ADR 0065): code sits in a `well` in the mono face,
the same surface as a command's output; inline code is the mono face at body size on a `well`
background; a quote is a `well` bar; rules and table lines are the `well` tone; links take the
accent. Headings are bold at body size. The Mac draws in `MarkdownViews.swift`, the
desktop in `views/markdown.rs`. The desktop opens a link in the system browser; the Mac routes
it to its web pane.

## Consequences

- The Markdown subset is defined once. Adding a construct means adding vectors and making both
  clients pass them.
- Apple builds gain two small dependencies (swift-markdown and swift-cmark); the desktop gains
  `pulldown-cmark`.
- The desktop embeds the italic and bold italic cuts of Inter, instanced from the Mac's
  variable font at its full slant by `scripts/instance_inter_fonts.py`, so `*emphasis*` is
  drawn in an italic face on every platform.

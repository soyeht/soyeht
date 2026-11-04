# ADR 0139: The Android app draws the Markdown blocks the core parses

- Status: accepted
- Date: 2026-11-04

## Context

ADR 0087 sets the Markdown subset every client draws for an agent's replies and thoughts, as data:
`contracts/vectors/markdown.json`, which the Mac and iPhone pass through `MarkdownReply.parse` and the
desktop passes through `soyeht_client::markdown::parse`. The first Android phase drew a reply as raw text,
asterisks and backticks included. A third hand-written parser in Kotlin would be a third copy of a rule
that already has two, and the one reason the Android app is a view over the Rust core (ADR 0134) is to
avoid exactly that.

## Decision

- **The parse is the desktop client's.** The core exports `markdown_blocks(text, streaming)`: it runs
  `soyeht_client::markdown::parse` (pulldown-cmark, with the closing of the spans a reply still arriving has
  left open) and then `link_blocks`, which links a bare web address by the rule of
  `contracts/vectors/web-links.json`. The blocks cross the boundary as plain UniFFI records and enums
  (`MarkdownBlock`, `MarkdownRun`), nested the way the vector's blocks are.
- **The vectors are passed through the boundary.** `markdown::tests::every_shared_vector_reaches_the_views_as_its_blocks`
  in the core turns what the exported function returns back into the vector's JSON shape and compares it with
  every case of `contracts/vectors/markdown.json`, so a change to the projection that drops a style, a
  list's first number or a code block's text fails the same file the other clients pass.
- **Kotlin only draws.** `MarkdownView` maps each block to Compose, flat and in palette roles
  (ADR 0087, "Drawing"): code in a `well` in the mono face, inline code on a `well` background, a quote a
  `well` bar, rules and tables in the `well` tone, links in the accent and underlined, headings bold at body
  size. A link opens through the system's URI handler and a phone with nothing to open it with is not
  disturbed. Only an agent's replies and thoughts are drawn this way; what a person typed, tool lines and
  system notices stay plain text.
- **A code block has a Copy button** at its top right. It copies the block's text as the parser kept it (no
  fence, no language tag) and shows "Copied" for `copiedFeedbackMs`. The words and the time are
  `x-clientPolicy`'s: the core exports them (`code_copy`), the view types none.
- **A reply that is still arriving** is parsed whole each time it grows, with `streaming` set. The app keeps
  the blocks of a message until its revision changes, as the other clients keep theirs, so a transcript that
  redraws does not parse the replies it already has.

## Not done

The desktop's page budget for very long replies ("Show more", `markdown::shown`) is not used yet: a long
reply is laid out whole. File references (`link_references`) open an editor, which the phone does not have.

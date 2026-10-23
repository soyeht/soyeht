//! How the Markdown of a text reads while it is written: which parts of the source a heading, a
//! bold or italic run, a quote, code or a link covers. The source stays as it is typed (the
//! Mac's text view hides the marks of the lines the caret is not on; the desktop shows them), and
//! the view draws each part in its look.

use pulldown_cmark::{Event, Options, Parser, Tag};
use std::ops::Range;

/// What a part of the source looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Heading,
    Strong,
    Emphasis,
    Code,
    Quote,
    Link,
}

/// The parts of `text` with a look, as byte ranges of it in the order they start. Parts nest
/// (a bold word in a heading), so ranges may overlap.
pub fn looks(text: &str) -> Vec<(Range<usize>, Look)> {
    let mut found = Vec::new();
    for (event, range) in Parser::new_ext(text, Options::empty()).into_offset_iter() {
        let look = match event {
            Event::Start(Tag::Heading { .. }) => Look::Heading,
            Event::Start(Tag::Strong) => Look::Strong,
            Event::Start(Tag::Emphasis) => Look::Emphasis,
            Event::Start(Tag::BlockQuote(_)) => Look::Quote,
            Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => Look::Code,
            Event::Start(Tag::Link { .. }) => Look::Link,
            _ => continue,
        };
        // A block's range takes the line break that ends it, which is not part of its look.
        let end = text[..range.end]
            .trim_end_matches('\n')
            .len()
            .max(range.start);
        found.push((range.start..end, look));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn looked(text: &str) -> Vec<(&str, Look)> {
        looks(text)
            .into_iter()
            .map(|(range, look)| (&text[range], look))
            .collect()
    }

    #[test]
    fn headings_runs_quotes_code_and_links_have_their_look() {
        let text = "# Title\n\nSome **bold** and *slanted* and `code` and [a link](https://x.y).\n\n> quoted\n\n    indented";
        let found = looked(text);
        assert!(found.contains(&("# Title", Look::Heading)), "{found:?}");
        assert!(found.contains(&("**bold**", Look::Strong)));
        assert!(found.contains(&("*slanted*", Look::Emphasis)));
        assert!(found.contains(&("`code`", Look::Code)));
        assert!(found.contains(&("[a link](https://x.y)", Look::Link)));
        assert!(
            found
                .iter()
                .any(|(part, look)| *look == Look::Quote && part.contains("quoted"))
        );
        assert!(
            found
                .iter()
                .any(|(part, look)| *look == Look::Code && part.contains("indented"))
        );
    }

    #[test]
    fn plain_text_has_no_look_and_an_empty_text_none_at_all() {
        assert!(looks("just words").is_empty());
        assert!(looks("").is_empty());
    }

    #[test]
    fn a_run_inside_a_heading_nests() {
        let found = looked("## A **big** day");
        assert_eq!(
            found,
            [
                ("## A **big** day", Look::Heading),
                ("**big**", Look::Strong)
            ]
        );
    }

    #[test]
    fn the_ranges_fall_on_characters() {
        let text = "\u{e9}**\u{4e2d}**\n\n# \u{1f600}";
        for (range, _) in looks(text) {
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
        }
    }
}

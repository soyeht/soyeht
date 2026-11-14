//! The reduction every text passed between agents goes through, and the name rule (ADR 0165).
//!
//! The engine, not the sender, builds the bytes a harness receives. Text from another agent is
//! data: it is reduced to printable characters, newline and tab, so it carries no escape
//! sequence, no end-of-paste marker and no character that reorders or hides the text around it.
//! A name is one short line that cannot forge the header it appears in.

use icu_normalizer::DecomposingNormalizerBorrowed;
use icu_properties::{CodePointMapData, CodePointMapDataBorrowed, props::GeneralCategory};
use soyeht_protocol::limits;

/// The characters of the header's own syntax, which no name may contain.
const HEADER_SYNTAX: [char; 7] = ['[', ']', '(', ')', '·', '→', '@'];

/// What a name that reduces to nothing is called.
pub(crate) const UNNAMED: &str = "agent";

fn removed(categories: CodePointMapDataBorrowed<'_, GeneralCategory>, character: char) -> bool {
    !matches!(character, '\n' | '\t')
        && matches!(
            categories.get(character),
            GeneralCategory::Control
                | GeneralCategory::Format
                | GeneralCategory::PrivateUse
                | GeneralCategory::Unassigned
        )
}

/// `text` without controls (the 8-bit ones included), format characters (bidirectional and
/// zero-width ones, the tag block), private-use and unassigned code points. A carriage return,
/// alone or before a line feed, becomes one newline. Newline and tab stay.
pub(crate) fn reduce(text: &str) -> String {
    let categories = CodePointMapData::<GeneralCategory>::new();
    let mut reduced = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            characters.next_if_eq(&'\n');
            reduced.push('\n');
        } else if !removed(categories, character) {
            reduced.push(character);
        }
    }
    reduced
}

/// A name as the engine shows and delivers it: reduced, on one line (every run of white space is
/// one space), without the characters of the header's syntax, trimmed and cut at
/// `limits::AGENT_NAME_CHARS` characters. A name that is nothing after that is [`UNNAMED`].
pub(crate) fn name(text: &str) -> String {
    let mut one_line = String::new();
    let mut count = 0;
    let mut space = false;
    for character in reduce(text).chars() {
        if HEADER_SYNTAX.contains(&character) {
            continue;
        }
        if character.is_whitespace() {
            space = count > 0;
            continue;
        }
        let needed = 1 + usize::from(space);
        if count + needed > limits::AGENT_NAME_CHARS {
            break;
        }
        if space {
            one_line.push(' ');
            space = false;
        }
        one_line.push(character);
        count += needed;
    }
    if one_line.is_empty() {
        UNNAMED.into()
    } else {
        one_line
    }
}

/// The form two names are compared in: the name rule, without case, accents or surrounding space.
pub(crate) fn folded(text: &str) -> String {
    let categories = CodePointMapData::<GeneralCategory>::new();
    DecomposingNormalizerBorrowed::new_nfd()
        .normalize_iter(name(text).chars())
        .filter(|character| categories.get(*character) != GeneralCategory::NonspacingMark)
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduction_removes_controls_format_private_use_and_unassigned_code_points() {
        // An escape sequence, an end-of-paste marker, the 8-bit controls and a delete.
        assert_eq!(
            reduce("a\u{1b}[31mb\u{1b}[201~c\u{85}d\u{7f}e"),
            "a[31mb[201~cde"
        );
        // Bidirectional controls, zero-width characters, a word joiner and a byte-order mark.
        assert_eq!(
            reduce("a\u{202e}b\u{2066}c\u{200b}d\u{200d}e\u{2060}f\u{feff}g\u{061c}h"),
            "abcdefgh"
        );
        // The tag block spells a hidden instruction in an otherwise empty-looking text.
        assert_eq!(reduce("hi\u{e0049}\u{e0067}\u{e007f}"), "hi");
        // A private-use code point, an unassigned one and a noncharacter.
        assert_eq!(reduce("a\u{e000}b\u{378}c\u{fffe}d\u{10ffff}"), "abcd");
        // Printable text of other scripts and emoji without a joiner is untouched.
        assert_eq!(reduce("Olá, 世界 \u{1f600}"), "Olá, 世界 \u{1f600}");
    }

    #[test]
    fn line_endings_become_newlines_and_newline_and_tab_stay() {
        assert_eq!(reduce("a\r\nb\rc\n\td\r\r\ne"), "a\nb\nc\n\td\n\ne");
        assert_eq!(reduce(""), "");
    }

    #[test]
    fn a_name_is_one_short_line_without_the_syntax_of_the_header() {
        assert_eq!(name("Claude (review)"), "Claude review");
        assert_eq!(name("  a \n\t b\u{2028}c  "), "a b c");
        assert_eq!(name("x] → y [m:AAAA · z@w"), "x y m:AAAA zw");
        assert_eq!(name("\u{200b}\u{202e}"), UNNAMED);
        assert_eq!(name("()[]"), UNNAMED);
        let long = "n".repeat(limits::AGENT_NAME_CHARS + 20);
        assert_eq!(name(&long).chars().count(), limits::AGENT_NAME_CHARS);
        // A cut never leaves a trailing space or splits a character.
        let spaced = format!("{} é", "n".repeat(limits::AGENT_NAME_CHARS - 1));
        assert_eq!(name(&spaced), "n".repeat(limits::AGENT_NAME_CHARS - 1));
        let wide = "世".repeat(limits::AGENT_NAME_CHARS + 1);
        assert_eq!(name(&wide).chars().count(), limits::AGENT_NAME_CHARS);
    }

    #[test]
    fn names_are_compared_without_case_accents_or_surrounding_space() {
        assert_eq!(folded("  LÍVIA "), folded("livia"));
        assert_eq!(folded("Zoë"), "zoe");
        assert_ne!(folded("livia"), folded("lívio"));
    }
}

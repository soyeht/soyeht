//! A text document as one string: the page of lines a file holds, joined by line breaks, and the
//! places of that string (a byte offset) as the engine names them (a line and a UTF-16 column).
//! A text field edits the string; the one edit that turns the page into what the field holds is
//! what the engine is sent.

use crate::editor::{
    page::{Page, position},
    text::column_at,
};
use crate::text_input::floor_boundary;
use soyeht_protocol::{TextPosition, TextRange};

/// The lines of the page, each followed by a line break but the last. The `\r` of a `\r\n` is part
/// of the break, not of the text (`Page`), so the string has `\n` breaks only.
pub fn text_of(page: &Page) -> String {
    let mut text = String::new();
    for (index, line) in page.lines.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        text.push_str(&line.text);
    }
    text
}

/// The place byte `at` of `text` stands at, for a document whose first line is `from_line`.
pub fn position_of(text: &str, from_line: u32, at: usize) -> TextPosition {
    let at = floor_boundary(text, at);
    let before = &text[..at];
    let breaks = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |found| found + 1);
    position(
        from_line.saturating_add(u32::try_from(breaks).unwrap_or(u32::MAX)),
        column_at(&text[line_start..], at - line_start),
    )
}

/// The range of the document that `anchor` and `head` (bytes of `text`) select, its start first.
pub fn range_of(text: &str, from_line: u32, anchor: usize, head: usize) -> TextRange {
    let (start, end) = if anchor <= head {
        (anchor, head)
    } else {
        (head, anchor)
    };
    TextRange {
        start: position_of(text, from_line, start),
        end: position_of(text, from_line, end),
    }
}

/// What replaces part of a document: the range, in the places of the document before, and the
/// text that takes its place.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub range: TextRange,
    pub text: String,
}

/// The one change that turns `before` into `after`: what lies between the text they share at the
/// start and the text they share at the end. `None` when they are the same.
pub fn change_between(before: &str, after: &str, from_line: u32) -> Option<Change> {
    if before == after {
        return None;
    }
    let shared = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let start = floor_boundary(before, shared);
    let room = before.len().min(after.len()) - start;
    let tail = before
        .bytes()
        .rev()
        .zip(after.bytes().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    // The shared tail begins where a character does in both texts: a byte that continues a
    // character is the same byte in each.
    let mut end_before = before.len() - tail;
    while !before.is_char_boundary(end_before) {
        end_before += 1;
    }
    let end_after = after.len() - (before.len() - end_before);
    Some(Change {
        range: TextRange {
            start: position_of(before, from_line, start),
            end: position_of(before, from_line, end_before),
        },
        text: after[start..end_after].to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::text::byte_at;

    /// The byte of `text` at a place, which is the end of the text for a place past it.
    pub fn offset_of(text: &str, from_line: u32, at: &TextPosition) -> usize {
        let Some(skipped) = at.line.checked_sub(from_line) else {
            return 0;
        };
        let mut start = 0;
        for _ in 0..skipped {
            match text[start..].find('\n') {
                Some(found) => start += found + 1,
                None => return text.len(),
            }
        }
        let end = text[start..]
            .find('\n')
            .map_or(text.len(), |found| start + found);
        start + byte_at(&text[start..end], at.column)
    }

    fn at(line: u32, column: u32) -> TextPosition {
        position(line, column)
    }

    #[test]
    fn the_page_is_one_string_with_a_line_break_between_lines() {
        let page = Page::of_text(0, "one\r\ntwo\n\nfour");
        assert_eq!(text_of(&page), "one\ntwo\n\nfour");
        assert_eq!(text_of(&Page::of_text(0, "")), "");
    }

    #[test]
    fn a_byte_stands_at_a_line_and_a_utf16_column() {
        let text = "ab\n\u{1f600}x\n";
        assert_eq!(position_of(text, 0, 0), at(0, 0));
        assert_eq!(position_of(text, 0, 2), at(0, 2));
        assert_eq!(position_of(text, 0, 3), at(1, 0));
        // The emoji takes two UTF-16 units.
        assert_eq!(position_of(text, 0, 7), at(1, 2));
        assert_eq!(position_of(text, 0, 8), at(1, 3));
        assert_eq!(position_of(text, 0, 9), at(2, 0));
        assert_eq!(position_of(text, 5, 3), at(6, 0));
        // A byte inside a character rounds down, one past the end is the end.
        assert_eq!(position_of(text, 0, 4), at(1, 0));
        assert_eq!(position_of(text, 0, 99), at(2, 0));
    }

    #[test]
    fn a_place_is_a_byte_and_the_two_agree() {
        let text = "ab\n\u{1f600}x\n";
        for byte in [0, 1, 2, 3, 7, 8, 9] {
            assert_eq!(offset_of(text, 0, &position_of(text, 0, byte)), byte);
        }
        assert_eq!(offset_of(text, 0, &at(9, 0)), text.len());
        assert_eq!(offset_of(text, 0, &at(1, 99)), 8);
        assert_eq!(offset_of(text, 5, &at(4, 0)), 0);
    }

    #[test]
    fn a_selection_is_a_range_whichever_way_it_was_made() {
        let text = "alpha\nbeta";
        let forward = range_of(text, 0, 2, 8);
        assert_eq!((&forward.start, &forward.end), (&at(0, 2), &at(1, 2)));
        assert_eq!(range_of(text, 0, 8, 2), forward);
    }

    fn apply(before: &str, change: &Change) -> String {
        let start = offset_of(before, 0, &change.range.start);
        let end = offset_of(before, 0, &change.range.end);
        format!("{}{}{}", &before[..start], change.text, &before[end..])
    }

    #[test]
    fn what_was_typed_is_the_one_change_between_two_texts() {
        let cases = [
            ("hello", "hello!", at(0, 5), at(0, 5), "!"),
            ("hello", "hell", at(0, 4), at(0, 5), ""),
            (
                "hello world",
                "hello brave world",
                at(0, 6),
                at(0, 6),
                "brave ",
            ),
            ("one\ntwo", "one\n\ntwo", at(1, 0), at(1, 0), "\n"),
            ("one\ntwo", "onetwo", at(0, 3), at(1, 0), ""),
            ("abc", "xyz", at(0, 0), at(0, 3), "xyz"),
            ("", "text", at(0, 0), at(0, 0), "text"),
        ];
        for (before, after, start, end, text) in cases {
            let change = change_between(before, after, 0).unwrap();
            assert_eq!(
                (
                    change.range.start.clone(),
                    change.range.end.clone(),
                    change.text.as_str()
                ),
                (start, end, text),
                "{before:?} -> {after:?}"
            );
            assert_eq!(apply(before, &change), after);
        }
        assert_eq!(change_between("same", "same", 0), None);
    }

    #[test]
    fn a_change_never_cuts_a_character() {
        // Two letters that share their first UTF-8 bytes, and an emoji next to its twin.
        for (before, after) in [
            ("a\u{e9}b", "a\u{e8}b"),
            ("\u{1f600}", "\u{1f601}"),
            ("x\u{1f600}\u{1f600}y", "x\u{1f600}y"),
            ("\u{4e2d}\u{6587}", "\u{4e2d}\u{6587}\u{6587}"),
        ] {
            let change = change_between(before, after, 0).unwrap();
            assert_eq!(apply(before, &change), after, "{before:?} -> {after:?}");
        }
    }

    #[test]
    fn a_change_is_named_in_the_lines_of_the_document() {
        let change = change_between("a\nb", "a\nbc", 40).unwrap();
        assert_eq!(change.range.start, at(41, 1));
    }
}

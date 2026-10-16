//! The text of one line in the three units the editor counts in: the engine's UTF-16 columns,
//! the bytes of a Rust string and the cells of the screen. A column inside a surrogate pair rounds
//! down to the start of the pair, as the Mac's text view does.
//!
//! The caret moves over a grapheme cluster (a flag, a family, a letter with its accents, a Hangul
//! syllable) and never splits one. A cluster takes the cells the terminal gives it: two for a wide
//! character or an emoji, one for a letter with its accents, none for a control character; a tab
//! runs to the next multiple of the tab width (`x-clientPolicy` `editor.tabWidth`). A line holds
//! no line break: `Page` keeps a `\r` before `\n` apart from the text.

use soyeht_protocol::{HighlightKind, HighlightSpan, client_policy::EDITOR_TAB_WIDTH};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};
use unicode_width::UnicodeWidthStr;

const TAB: usize = EDITOR_TAB_WIDTH as usize;
/// How many spans back from a column are looked at for one that covers it.
const NESTED_SPANS: usize = 8;
/// The most cells a character takes.
const WIDEST: usize = 2;

pub fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

fn units(scalar: char) -> u32 {
    u32::try_from(scalar.len_utf16()).unwrap_or(1)
}

/// The byte offset of UTF-16 `column`: the end of the text for a column past it, the start of
/// the pair for a column inside one.
pub fn byte_at(text: &str, column: u32) -> usize {
    let mut passed = 0u32;
    for (byte, scalar) in text.char_indices() {
        let next = passed.saturating_add(units(scalar));
        if passed >= column || next > column {
            return byte;
        }
        passed = next;
    }
    text.len()
}

/// The UTF-16 column of a byte offset.
pub fn column_at(text: &str, byte: usize) -> u32 {
    text.get(..byte).map_or_else(|| utf16_len(text), utf16_len)
}

/// The byte after the character that starts at `start`: a whole grapheme cluster.
pub fn cluster_end(text: &str, start: usize) -> usize {
    if start >= text.len() {
        return text.len();
    }
    match GraphemeCursor::new(start, text.len(), true).next_boundary(text, 0) {
        Ok(Some(end)) => end,
        _ => text.len(),
    }
}

/// The byte where the character that ends at `end` starts.
pub fn cluster_start(text: &str, end: usize) -> usize {
    if end == 0 || end > text.len() {
        return 0;
    }
    match GraphemeCursor::new(end, text.len(), true).prev_boundary(text, 0) {
        Ok(Some(start)) => start,
        _ => 0,
    }
}

/// The cells a cluster takes when it starts at cell `cell`.
fn width(cluster: &str, cell: usize) -> usize {
    if cluster == "\t" {
        TAB - cell % TAB
    } else if cluster.chars().all(char::is_control) {
        // A control character, and the `\r` that a line that was cut at `\n` may still have.
        0
    } else {
        UnicodeWidthStr::width(cluster).min(WIDEST)
    }
}

/// The cell where UTF-16 `column` starts.
pub fn cells_before(text: &str, column: u32) -> usize {
    let (mut passed, mut cell) = (0u32, 0usize);
    for cluster in text.graphemes(true) {
        if passed >= column {
            break;
        }
        passed = passed.saturating_add(utf16_len(cluster));
        cell += width(cluster, cell);
    }
    cell
}

/// The cells a line takes.
pub fn line_cells(text: &str) -> usize {
    text.graphemes(true)
        .fold(0, |cell, cluster| cell + width(cluster, cell))
}

/// The UTF-16 column nearest to `cell`: a click in the right half of a character lands after it.
pub fn column_at_cell(text: &str, cell: usize) -> u32 {
    let (mut passed, mut at) = (0u32, 0usize);
    for cluster in text.graphemes(true) {
        let size = width(cluster, at);
        if size > 0 && cell < at + size {
            return if (cell - at) * 2 >= size {
                passed.saturating_add(utf16_len(cluster))
            } else {
                passed
            };
        }
        passed = passed.saturating_add(utf16_len(cluster));
        at += size;
    }
    passed
}

/// A piece of a line as it is drawn, at a cell counted from the first one shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub at: usize,
    pub text: String,
    /// Runs of equal highlight, each with its length in bytes.
    pub runs: Vec<(usize, Option<HighlightKind>)>,
    /// Plain one-cell characters, which are laid on the grid. A wide character or one with
    /// accents is a part of its own, drawn from its cell with the width its glyphs have.
    pub spread: bool,
}

/// A slice of a line as it is drawn: tabs spelled as example, control characters left out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shown {
    pub parts: Vec<Part>,
}

/// The highlight that covers UTF-16 `column`, when one does. Spans come in order of their start;
/// of two that cover the column the later one wins.
fn kind_at(highlights: &[HighlightSpan], column: u32) -> Option<HighlightKind> {
    let reach = highlights.partition_point(|span| span.start_column <= column);
    highlights[..reach]
        .iter()
        .rev()
        .take(NESTED_SPANS)
        .find(|span| column < span.end_column)
        .map(|span| span.kind.clone())
}

impl Shown {
    /// Adds `text` of one kind at `cell`, joining the part before it when both are on the grid
    /// and it ends where this one starts.
    fn push(&mut self, cell: usize, text: &str, kind: Option<HighlightKind>, spread: bool) {
        let bytes = text.len();
        if spread && let Some(last) = self.parts.last_mut() {
            let reach = last.at + last.text.chars().count();
            if last.spread && reach == cell {
                last.text.push_str(text);
                match last.runs.last_mut() {
                    Some((length, before)) if *before == kind => *length += bytes,
                    _ => last.runs.push((bytes, kind)),
                }
                return;
            }
        }
        self.parts.push(Part {
            at: cell,
            text: text.to_owned(),
            runs: vec![(bytes, kind)],
            spread,
        });
    }
}

/// Cells `first..first + count` of a line, so that a line of a megabyte costs what the window
/// shows of it. A character that is cut by the left edge is left out.
pub fn show(text: &str, highlights: &[HighlightSpan], first: usize, count: usize) -> Shown {
    let mut shown = Shown::default();
    let (mut passed, mut cell) = (0u32, 0usize);
    for cluster in text.graphemes(true) {
        let size = width(cluster, cell);
        let column = passed;
        passed = passed.saturating_add(utf16_len(cluster));
        let start = cell;
        cell += size;
        if size == 0 || start < first {
            continue;
        }
        if start >= first + count {
            break;
        }
        let kind = kind_at(highlights, column);
        let at = start - first;
        if cluster == "\t" {
            let visible = size.min(first + count - start);
            shown.push(at, &" ".repeat(visible), kind, true);
        } else if size == 1 && cluster.chars().count() == 1 {
            shown.push(at, cluster, kind, true);
        } else {
            shown.push(at, cluster, kind, false);
        }
    }
    shown
}

/// What a character is for moving by words and for selecting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Other,
}

fn class(scalar: char) -> Class {
    if scalar.is_alphanumeric() || scalar == '_' {
        Class::Word
    } else if scalar.is_whitespace() {
        Class::Space
    } else {
        Class::Other
    }
}

fn scalar_before(text: &str, at: usize) -> Option<char> {
    text.get(..at).and_then(|head| head.chars().next_back())
}

fn scalar_after(text: &str, at: usize) -> Option<char> {
    text.get(at..).and_then(|rest| rest.chars().next())
}

/// Moves `at` back over scalars of class `kind`.
fn back_over(text: &str, mut at: usize, kind: Class) -> usize {
    while let Some(scalar) = scalar_before(text, at).filter(|scalar| class(*scalar) == kind) {
        at -= scalar.len_utf8();
    }
    at
}

/// Moves `at` forward over scalars of class `kind`.
fn forward_over(text: &str, mut at: usize, kind: Class) -> usize {
    while let Some(scalar) = scalar_after(text, at).filter(|scalar| class(*scalar) == kind) {
        at += scalar.len_utf8();
    }
    at
}

/// The column where the word before `column` starts: the whitespace before the caret is skipped,
/// then the run of the same kind of character.
pub fn word_before(text: &str, column: u32) -> u32 {
    let at = back_over(text, byte_at(text, column), Class::Space);
    let Some(scalar) = scalar_before(text, at) else {
        return 0;
    };
    column_at(text, back_over(text, at, class(scalar)))
}

/// The column where the word after `column` ends: the run of the same kind of character, then
/// the whitespace after it.
pub fn word_after(text: &str, column: u32) -> u32 {
    let mut at = byte_at(text, column);
    if let Some(scalar) = scalar_after(text, at) {
        at = forward_over(text, at, class(scalar));
    }
    column_at(text, forward_over(text, at, Class::Space))
}

/// The columns of the word under `column`, as a double click selects it; the run of example or
/// of punctuation when it is not on a word.
pub fn word_at(text: &str, column: u32) -> (u32, u32) {
    let at = byte_at(text, column);
    let on = scalar_after(text, at).map(class);
    let before = scalar_before(text, at).map(class);
    let kind = match (on, before) {
        (Some(Class::Word), _) | (_, Some(Class::Word)) => Class::Word,
        (Some(kind), _) | (None, Some(kind)) => kind,
        (None, None) => return (column, column),
    };
    (
        column_at(text, back_over(text, at, kind)),
        column_at(text, forward_over(text, at, kind)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u32, end: u32, kind: HighlightKind) -> HighlightSpan {
        HighlightSpan {
            start_column: start,
            end_column: end,
            kind,
        }
    }

    fn drawn(shown: &Shown) -> String {
        shown.parts.iter().map(|part| part.text.as_str()).collect()
    }

    #[test]
    fn columns_count_utf16_units_and_round_down_inside_a_pair() {
        let text = "a\u{1F600}b";
        assert_eq!(utf16_len(text), 4);
        assert_eq!(byte_at(text, 1), 1);
        assert_eq!(byte_at(text, 2), 1);
        assert_eq!(byte_at(text, 3), 5);
        assert_eq!(byte_at(text, 99), text.len());
        assert_eq!(column_at(text, 5), 3);
    }

    #[test]
    fn a_character_is_a_whole_grapheme_cluster() {
        for cluster in [
            "e\u{301}",
            "\u{1F1E7}\u{1F1F7}",
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
            "\u{1112}\u{1161}\u{11AB}",
            "\u{1F44D}\u{1F3FD}",
            "\u{6F22}",
        ] {
            let text = format!("a{cluster}b");
            let end = cluster_end(&text, 1);
            assert_eq!(&text[1..end], cluster);
            assert_eq!(cluster_start(&text, end), 1);
        }
        assert_eq!(cluster_end("ab", 2), 2);
        assert_eq!(cluster_start("ab", 0), 0);
    }

    #[test]
    fn cells_follow_the_width_of_each_cluster() {
        assert_eq!(line_cells("a\tb"), TAB + 1);
        assert_eq!(cells_before("a\tb", 2), TAB);
        assert_eq!(line_cells("a\u{6F22}b"), 4);
        assert_eq!(line_cells("\u{1F1E7}\u{1F1F7}"), 2);
        assert_eq!(line_cells("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"), 2);
        assert_eq!(line_cells("e\u{301}"), 1);
        assert_eq!(line_cells("\u{1112}\u{1161}\u{11AB}"), 2);
        assert_eq!(line_cells("a\u{7}b"), 2);
        assert_eq!(cells_before("a\u{301}b", 2), 1);
        assert_eq!(cells_before("x\u{6F22}y", 2), 3);
    }

    #[test]
    fn a_click_lands_on_the_nearest_edge_of_a_cluster() {
        assert_eq!(column_at_cell("a\tb", TAB), 2);
        assert_eq!(column_at_cell("abc", 1), 1);
        assert_eq!(column_at_cell("abc", 99), 3);
        assert_eq!(column_at_cell("\tx", 1), 0);
        assert_eq!(column_at_cell("\tx", TAB - 1), 1);
        assert_eq!(column_at_cell("a\u{6F22}b", 1), 1);
        assert_eq!(column_at_cell("a\u{6F22}b", 2), 2);
        assert_eq!(column_at_cell("a\u{6F22}b", 3), 2);
    }

    #[test]
    fn shown_cells_carry_their_highlights_and_spell_tabs_as_example() {
        let marks = [span(0, 2, HighlightKind::Keyword)];
        let shown = show("fn\tx", &marks, 0, 80);
        assert_eq!(shown.parts.len(), 1);
        assert_eq!(shown.parts[0].text, "fn  x");
        assert_eq!(
            shown.parts[0].runs,
            [(2, Some(HighlightKind::Keyword)), (3, None)]
        );
        assert!(shown.parts[0].spread);
    }

    #[test]
    fn a_letter_with_accents_and_a_wide_character_are_drawn_each_from_its_cell() {
        let shown = show("ae\u{301}\u{6F22}b\r", &[], 0, 80);
        let parts: Vec<_> = shown
            .parts
            .iter()
            .map(|part| (part.at, part.text.as_str(), part.spread))
            .collect();
        assert_eq!(
            parts,
            [
                (0, "a", true),
                (1, "e\u{301}", false),
                (2, "\u{6F22}", false),
                (4, "b", true)
            ]
        );
    }

    #[test]
    fn a_window_of_a_long_line_holds_only_what_it_shows() {
        let line = "0123456789".repeat(1000);
        let shown = show(&line, &[], 5, 10);
        assert_eq!(drawn(&shown), "5678901234");
        assert_eq!(shown.parts[0].at, 0);
        // A tab that the left edge cuts is blank anyway, and is left out.
        assert_eq!(drawn(&show("a\tb", &[], 2, 2)), "");
        assert_eq!(drawn(&show("a\u{6F22}b", &[], 2, 5)), "b");
    }

    #[test]
    fn words_are_runs_of_one_kind_of_character() {
        let text = "let value = foo_bar;";
        assert_eq!(word_after(text, 0), 4);
        assert_eq!(word_after(text, 4), 10);
        assert_eq!(word_before(text, 9), 4);
        assert_eq!(word_before(text, 4), 0);
        assert_eq!(word_before(text, 0), 0);
        assert_eq!(word_at(text, 6), (4, 9));
        assert_eq!(word_at(text, 9), (4, 9));
        assert_eq!(word_at(text, 3), (0, 3));
        assert_eq!(word_at("", 0), (0, 0));
    }
}

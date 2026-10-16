//! The page of lines a file has loaded: the part of the document the editor shows and edits. The
//! engine keeps the whole file; a page is at most `limits::EDITOR_VIEWPORT_LINES` lines of it, and
//! a keystroke changes the page at once while the engine is told.

use super::text::{byte_at, utf16_len};
use soyeht_protocol::{FileReadResult, HighlightSpan, TextEdit, TextPosition, TextRange};
use std::cmp::Ordering;

/// The line break that ends a line when the file ends its lines with `\r\n`.
pub const CRLF: &str = "\r\n";
pub const LF: &str = "\n";

/// One line of the page. The engine keeps the `\r` of a `\r\n` at the end of the line's text; here
/// it is part of the line break, not of the text, so that the end of the line is before it, a
/// join removes it and a new line breaks the way the file's lines do.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Line {
    pub text: String,
    pub highlights: Vec<HighlightSpan>,
    /// The line ends with `\r\n`.
    pub crlf: bool,
}

impl Line {
    /// A line of `text`, which may end with the `\r` of a `\r\n`.
    pub fn new(text: &str, highlights: Vec<HighlightSpan>) -> Self {
        let (text, crlf) = text
            .strip_suffix('\r')
            .map_or((text, false), |text| (text, true));
        Self {
            text: text.to_owned(),
            highlights,
            crlf,
        }
    }

    pub fn plain(text: &str) -> Self {
        Self::new(text, Vec::new())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub from_line: u32,
    pub lines: Vec<Line>,
    /// The revision of the file the lines were read at, or last edited to.
    pub revision: String,
    /// How many lines were asked for when the page was read (`Window::asked_lines`).
    pub asked: u32,
}

pub fn position(line: u32, column: u32) -> TextPosition {
    TextPosition { line, column }
}

/// Where `a` stands against `b` in a document.
pub fn order(a: &TextPosition, b: &TextPosition) -> Ordering {
    (a.line, a.column).cmp(&(b.line, b.column))
}

/// The two ends of a selection, first the one that comes first.
pub fn ordered(a: &TextPosition, b: &TextPosition) -> (TextPosition, TextPosition) {
    if order(a, b).is_gt() {
        (b.clone(), a.clone())
    } else {
        (a.clone(), b.clone())
    }
}

impl Page {
    pub fn from_read(result: &FileReadResult) -> Self {
        Self {
            from_line: result.from_line,
            lines: result
                .lines
                .iter()
                .map(|line| Line::new(&line.text, line.highlights.clone()))
                .collect(),
            revision: result.file.revision.clone(),
            asked: u32::try_from(result.lines.len()).unwrap_or(u32::MAX),
        }
    }

    /// A page of `text` (its lines joined by a newline) that starts at `from_line`.
    #[cfg(test)]
    pub fn of_text(from_line: u32, text: &str) -> Self {
        Self {
            from_line,
            lines: text.split('\n').map(Line::plain).collect(),
            revision: String::new(),
            asked: 0,
        }
    }

    pub fn len(&self) -> u32 {
        u32::try_from(self.lines.len()).unwrap_or(u32::MAX)
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The line after the last one the page holds.
    pub fn to_line(&self) -> u32 {
        self.from_line.saturating_add(self.len())
    }

    pub fn holds(&self, line: u32) -> bool {
        line >= self.from_line && line < self.to_line()
    }

    pub fn line(&self, line: u32) -> Option<&Line> {
        let index = line.checked_sub(self.from_line)?;
        self.lines.get(index as usize)
    }

    /// The text of the line, empty for a line the page does not hold.
    pub fn text(&self, line: u32) -> &str {
        self.line(line).map_or("", |line| line.text.as_str())
    }

    /// The line break the file's lines have: the one of the line, or for the last line of the
    /// page, which may be the last of the file and then has none, the one of the line before.
    pub fn ending(&self, line: u32) -> &'static str {
        let crlf = |line: u32| self.line(line).is_some_and(|line| line.crlf);
        let last = line.saturating_add(1) >= self.to_line();
        let before = line > self.from_line && crlf(line - 1);
        if crlf(line) || (last && before) {
            CRLF
        } else {
            LF
        }
    }

    /// The end of the line, in UTF-16 columns.
    pub fn line_end(&self, line: u32) -> u32 {
        utf16_len(self.text(line))
    }

    /// The position, moved to a line the page holds and a column the line has.
    pub fn clamp(&self, at: &TextPosition) -> TextPosition {
        let last = self.to_line().saturating_sub(1).max(self.from_line);
        let line = at.line.clamp(self.from_line, last);
        position(line, at.column.min(self.line_end(line)))
    }

    pub fn first(&self) -> TextPosition {
        position(self.from_line, 0)
    }

    pub fn last(&self) -> TextPosition {
        let line = self.to_line().saturating_sub(1).max(self.from_line);
        position(line, self.line_end(line))
    }

    /// The text between two positions the page holds, each line followed by its own break.
    pub fn between(&self, from: &TextPosition, to: &TextPosition) -> String {
        let (from, to) = ordered(from, to);
        let mut out = String::new();
        for line in from.line..=to.line {
            let text = self.text(line);
            let start = if line == from.line {
                byte_at(text, from.column)
            } else {
                0
            };
            let end = if line == to.line {
                byte_at(text, to.column)
            } else {
                text.len()
            };
            out.push_str(text.get(start..end).unwrap_or_default());
            if line < to.line {
                out.push_str(if self.line(line).is_some_and(|line| line.crlf) {
                    CRLF
                } else {
                    LF
                });
            }
        }
        out
    }

    /// Replaces `range` with `text`. False, and nothing changed, when the range reaches a line the
    /// page does not hold. The highlights of an edit inside one line stay with the text they
    /// were on, so a keystroke does not strip the color of its line; those of a line that
    /// is joined or split are dropped until the page is read again.
    pub fn replace(&mut self, range: &TextRange, text: &str) -> bool {
        let (start, end) = (&range.start, &range.end);
        if order(start, end).is_gt() || !self.holds(start.line) || !self.holds(end.line) {
            return false;
        }
        let first = (start.line - self.from_line) as usize;
        let last = (end.line - self.from_line) as usize;
        let head = &self.lines[first].text;
        let tail = &self.lines[last].text;
        let tail_crlf = self.lines[last].crlf;
        let prefix = head.get(..byte_at(head, start.column)).unwrap_or_default();
        let suffix = tail.get(byte_at(tail, end.column)..).unwrap_or_default();
        let merged = format!("{prefix}{text}{suffix}");
        let within_a_line = first == last && !text.contains('\n');
        let kept = within_a_line.then(|| {
            shifted(
                &self.lines[first].highlights,
                start.column,
                end.column,
                utf16_len(text),
            )
        });
        let pieces: Vec<&str> = merged.split('\n').collect();
        let replacement: Vec<Line> = pieces
            .iter()
            .enumerate()
            .map(|(index, piece)| {
                let mut line = Line::new(piece, kept.clone().unwrap_or_default());
                // The last piece ends where the line it joined ended.
                if index + 1 == pieces.len() {
                    line.text = (*piece).to_owned();
                    line.crlf = tail_crlf;
                }
                line
            })
            .collect();
        self.lines.splice(first..=last, replacement);
        true
    }

    /// Makes an edit the engine is sent in the page too.
    pub fn apply(&mut self, edit: &TextEdit) -> bool {
        self.replace(&edit.range, &edit.text)
    }
}

/// The spans of a line after `start..end` of it was replaced by `inserted` UTF-16 units.
fn shifted(spans: &[HighlightSpan], start: u32, end: u32, inserted: u32) -> Vec<HighlightSpan> {
    let removed = end - start;
    let moved = |column: u32| column - removed + inserted;
    spans
        .iter()
        .filter_map(|span| {
            let from = match span.start_column {
                column if column <= start => column,
                column if column >= end => moved(column),
                _ => start + inserted,
            };
            let to = match span.end_column {
                column if column <= start => column,
                column if column >= end => moved(column),
                _ => start,
            };
            (from < to).then(|| HighlightSpan {
                start_column: from,
                end_column: to,
                kind: span.kind.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use soyeht_protocol::HighlightKind;

    fn range(a: (u32, u32), b: (u32, u32)) -> TextRange {
        TextRange {
            start: position(a.0, a.1),
            end: position(b.0, b.1),
        }
    }

    #[test]
    fn a_page_holds_the_lines_it_starts_at() {
        let page = Page::of_text(10, "one\ntwo\nthree");
        assert!(page.holds(10) && page.holds(12) && !page.holds(9) && !page.holds(13));
        assert_eq!(page.text(11), "two");
        assert_eq!(page.text(40), "");
        assert_eq!(page.to_line(), 13);
        assert_eq!(page.clamp(&position(99, 99)), position(12, 5));
        assert_eq!(page.clamp(&position(0, 1)), position(10, 1));
        assert_eq!(page.last(), position(12, 5));
    }

    #[test]
    fn text_between_two_positions_joins_the_lines() {
        let page = Page::of_text(0, "alpha\nbeta\ngamma");
        assert_eq!(
            page.between(&position(0, 3), &position(2, 2)),
            "ha\nbeta\nga"
        );
        assert_eq!(
            page.between(&position(2, 2), &position(0, 3)),
            "ha\nbeta\nga"
        );
        assert_eq!(page.between(&position(1, 1), &position(1, 1)), "");
        let mixed = Page::of_text(0, "ab\r\ncd\nef");
        assert_eq!(
            mixed.between(&position(0, 1), &position(2, 1)),
            "b\r\ncd\ne"
        );
    }

    #[test]
    fn a_replacement_may_join_and_split_lines() {
        let mut page = Page::of_text(5, "abc\ndef\nghi");
        assert!(page.replace(&range((5, 1), (6, 2)), "X\nY\nZ"));
        let text: Vec<_> = page.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(text, ["aX", "Y", "Zf", "ghi"]);
        assert!(!page.replace(&range((4, 0), (5, 0)), "x"));
        assert!(!page.replace(&range((9, 0), (9, 0)), "x"));
        assert!(!page.replace(&range((6, 1), (5, 0)), "x"));
    }

    #[test]
    fn the_line_break_of_a_line_is_not_part_of_its_text() {
        let page = Page::of_text(0, "abc\r\ndef\r\nghi");
        assert_eq!(page.text(0), "abc");
        assert_eq!(page.line_end(0), 3);
        assert_eq!(page.clamp(&position(0, 9)), position(0, 3));
        assert!(page.line(0).unwrap().crlf && !page.line(2).unwrap().crlf);
        let mut page = page;
        // Joining two lines takes the break between them, and a new line breaks like the others.
        assert!(page.replace(&range((0, 3), (1, 0)), ""));
        assert_eq!(page.text(0), "abcdef");
        assert!(page.line(0).unwrap().crlf);
        let ending = page.ending(0);
        assert!(page.replace(&range((0, 6), (0, 6)), ending));
        assert_eq!(page.text(1), "");
        assert!(page.line(0).unwrap().crlf && page.line(1).unwrap().crlf);
    }

    #[test]
    fn the_line_ending_is_the_one_the_vector_says() {
        for case in crate::testing::cases("editor", "lineEndings") {
            let lines: Vec<&str> = case["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|line| line.as_str().unwrap())
                .collect();
            let page = Page::of_text(0, &lines.join("\n"));
            let line = u32::try_from(case["line"].as_u64().unwrap()).unwrap();
            assert_eq!(
                page.ending(line),
                case["ending"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn a_keystroke_keeps_the_color_of_its_line() {
        let mut page = Page::of_text(0, "let x = 1");
        page.lines[0].highlights = vec![
            HighlightSpan {
                start_column: 0,
                end_column: 3,
                kind: HighlightKind::Keyword,
            },
            HighlightSpan {
                start_column: 8,
                end_column: 9,
                kind: HighlightKind::Number,
            },
        ];
        // Typing inside the keyword extends it; the number moves with the text.
        assert!(page.replace(&range((0, 2), (0, 2)), "tt"));
        assert_eq!(page.lines[0].text, "lettt x = 1");
        let spans: Vec<_> = page.lines[0]
            .highlights
            .iter()
            .map(|span| (span.start_column, span.end_column))
            .collect();
        assert_eq!(spans, [(0, 5), (10, 11)]);
        // Deleting the keyword's tail clips it; deleting all of a span drops it.
        assert!(page.replace(&range((0, 1), (0, 5)), ""));
        assert_eq!(page.lines[0].highlights[0].end_column, 1);
        assert!(page.replace(&range((0, 0), (0, 1)), ""));
        assert_eq!(page.lines[0].highlights.len(), 1);
    }
}

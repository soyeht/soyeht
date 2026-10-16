//! The caret and the selection, and where each motion takes them.

use super::{
    page::{Page, ordered, position},
    text::{
        byte_at, cells_before, cluster_end, cluster_start, column_at, column_at_cell, word_after,
        word_before,
    },
};
use soyeht_protocol::{TextPosition, TextRange};

/// The end of any line, for a motion that does not know the line's length yet: a position is
/// clamped to its line when it is placed.
pub const LINE_END: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    /// Where the selection started.
    pub anchor: TextPosition,
    /// Where the caret is.
    pub head: TextPosition,
    /// The cell the caret keeps while it moves up and down through lines of different lengths.
    desired: Option<usize>,
}

impl Selection {
    pub fn caret(at: TextPosition) -> Self {
        Self {
            anchor: at.clone(),
            head: at,
            desired: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The selected range, its start first.
    pub fn range(&self) -> TextRange {
        let (start, end) = ordered(&self.anchor, &self.head);
        TextRange { start, end }
    }

    /// The selection with its caret at `head`; the anchor stays when the selection grows.
    pub fn moved_to(&self, head: TextPosition, extend: bool) -> Self {
        Self {
            anchor: if extend {
                self.anchor.clone()
            } else {
                head.clone()
            },
            head,
            desired: None,
        }
    }

    pub fn selecting(anchor: TextPosition, head: TextPosition) -> Self {
        Self {
            anchor,
            head,
            desired: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    WordLeft,
    WordRight,
    DocumentStart,
    DocumentEnd,
    PageUp(u32),
    PageDown(u32),
}

impl Motion {
    fn is_vertical(self) -> bool {
        matches!(
            self,
            Self::Up | Self::Down | Self::PageUp(_) | Self::PageDown(_)
        )
    }
}

/// Where a motion takes the caret, in a document of `total` lines. A line the page does not hold
/// is named all the same, with `LINE_END` for a column that is not known; the caller loads the
/// page that holds it.
pub fn target(
    page: &Page,
    total: u32,
    from: &Selection,
    motion: Motion,
    extend: bool,
) -> Selection {
    let head = &from.head;
    let text = page.text(head.line);
    let last = total.saturating_sub(1);
    let collapse = !extend && !from.is_empty();
    let to = match motion {
        Motion::Left if collapse => from.range().start,
        Motion::Right if collapse => from.range().end,
        Motion::Left => match head.column {
            0 if head.line > 0 => position(head.line - 1, LINE_END),
            0 => head.clone(),
            column => {
                let end = byte_at(text, column);
                position(head.line, column_at(text, cluster_start(text, end)))
            }
        },
        Motion::Right => {
            let at = byte_at(text, head.column);
            if at < text.len() {
                position(head.line, column_at(text, cluster_end(text, at)))
            } else if head.line < last {
                position(head.line + 1, 0)
            } else {
                head.clone()
            }
        }
        Motion::LineStart => position(head.line, 0),
        Motion::LineEnd => position(head.line, LINE_END),
        Motion::WordLeft => match head.column {
            0 if head.line > 0 => position(head.line - 1, LINE_END),
            column => position(head.line, word_before(text, column)),
        },
        Motion::WordRight => {
            if byte_at(text, head.column) >= text.len() && head.line < last {
                position(head.line + 1, 0)
            } else {
                position(head.line, word_after(text, head.column))
            }
        }
        Motion::DocumentStart => position(0, 0),
        Motion::DocumentEnd => position(last, LINE_END),
        Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_) => {
            return vertical(page, total, from, motion, extend);
        }
    };
    from.moved_to(to, extend)
}

fn vertical(page: &Page, total: u32, from: &Selection, motion: Motion, extend: bool) -> Selection {
    let head = &from.head;
    let last = total.saturating_sub(1);
    let line = match motion {
        Motion::PageUp(rows) => head.line.saturating_sub(rows.max(1)),
        Motion::PageDown(rows) => head.line.saturating_add(rows.max(1)).min(last),
        Motion::Up => head.line.saturating_sub(1),
        _ => (head.line + 1).min(last),
    };
    let up = matches!(motion, Motion::Up | Motion::PageUp(_));
    let cell = from
        .desired
        .unwrap_or_else(|| cells_before(page.text(head.line), head.column));
    // The first line has nothing above it and the last nothing below: the caret goes to the end.
    let to = if line == head.line && head.line == 0 && up {
        position(0, 0)
    } else if line == head.line {
        position(line, LINE_END)
    } else if page.holds(line) {
        position(line, column_at_cell(page.text(line), cell))
    } else {
        position(line, LINE_END)
    };
    let mut moved = from.moved_to(to, extend);
    moved.desired = (motion.is_vertical()).then_some(cell);
    moved
}

/// The selection with its caret clamped to the page: a line the page holds, a column it has.
pub fn clamped(page: &Page, selection: &Selection) -> Selection {
    Selection {
        anchor: page.clamp(&selection.anchor),
        head: page.clamp(&selection.head),
        desired: selection.desired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Page {
        Page::of_text(0, "alpha beta\n\nxy\nlonger line here")
    }

    fn at(line: u32, column: u32) -> Selection {
        Selection::caret(position(line, column))
    }

    fn go(from: Selection, motion: Motion) -> Selection {
        target(&page(), 4, &from, motion, false)
    }

    #[test]
    fn left_and_right_cross_lines_and_skip_whole_characters() {
        assert_eq!(go(at(0, 3), Motion::Left).head, position(0, 2));
        assert_eq!(go(at(2, 0), Motion::Left).head, position(1, LINE_END));
        assert_eq!(go(at(0, 0), Motion::Left).head, position(0, 0));
        assert_eq!(go(at(0, 10), Motion::Right).head, position(1, 0));
        assert_eq!(go(at(3, 16), Motion::Right).head, position(3, 16));
        let emoji = Page::of_text(0, "a\u{1F600}b");
        let right = target(&emoji, 1, &at(0, 1), Motion::Right, false);
        assert_eq!(right.head, position(0, 3));
        assert_eq!(
            target(&emoji, 1, &right, Motion::Left, false).head,
            position(0, 1)
        );
    }

    #[test]
    fn left_and_right_collapse_a_selection_to_its_edges() {
        let selected = Selection::selecting(position(0, 2), position(0, 5));
        assert_eq!(go(selected.clone(), Motion::Left).head, position(0, 2));
        assert_eq!(go(selected.clone(), Motion::Right).head, position(0, 5));
        assert_eq!(
            target(&page(), 4, &selected, Motion::Right, true).anchor,
            position(0, 2)
        );
    }

    #[test]
    fn up_and_down_keep_the_column_through_a_short_line() {
        let down = go(at(0, 8), Motion::Down);
        assert_eq!(down.head, position(1, 0));
        let again = go(down, Motion::Down);
        assert_eq!(again.head, position(2, 2));
        let third = go(again, Motion::Down);
        assert_eq!(third.head, position(3, 8));
        assert_eq!(go(at(0, 4), Motion::Up).head, position(0, 0));
        assert_eq!(go(at(3, 4), Motion::Down).head, position(3, LINE_END));
    }

    #[test]
    fn a_line_the_page_does_not_hold_is_named_for_the_caller_to_load() {
        let far = Page::of_text(10, "a\nb");
        let up = target(&far, 100, &at(10, 0), Motion::Up, false);
        assert_eq!(up.head, position(9, LINE_END));
        let down = target(&far, 100, &at(11, 0), Motion::PageDown(40), false);
        assert_eq!(down.head.line, 51);
    }

    #[test]
    fn home_end_words_and_the_document_ends() {
        assert_eq!(go(at(0, 4), Motion::LineStart).head, position(0, 0));
        assert_eq!(go(at(0, 4), Motion::LineEnd).head, position(0, LINE_END));
        assert_eq!(go(at(0, 0), Motion::WordRight).head, position(0, 6));
        assert_eq!(go(at(0, 10), Motion::WordRight).head, position(1, 0));
        assert_eq!(go(at(0, 6), Motion::WordLeft).head, position(0, 0));
        assert_eq!(go(at(1, 0), Motion::WordLeft).head, position(0, LINE_END));
        assert_eq!(go(at(1, 0), Motion::DocumentStart).head, position(0, 0));
        assert_eq!(
            go(at(1, 0), Motion::DocumentEnd).head,
            position(3, LINE_END)
        );
    }

    #[test]
    fn extending_keeps_the_anchor_and_clamping_keeps_the_caret_in_the_page() {
        let grown = target(&page(), 4, &at(0, 2), Motion::LineEnd, true);
        assert_eq!(grown.anchor, position(0, 2));
        assert_eq!(clamped(&page(), &grown).head, position(0, 10));
    }
}

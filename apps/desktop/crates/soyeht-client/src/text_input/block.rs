//! The selection in the read-only text of the transcript: one block (a paragraph, a code block, a
//! command's output, a message) at a time, made by dragging over it, a double click on a word or a
//! triple click on a line. Choosing in a block ends the selection in the one before. A block is
//! named by the owner the view gives it; the text it holds is passed in with each gesture, so
//! the selection keeps the text it would copy even when the block scrolls out of the window.

use super::{floor_boundary, line_range, word_range};
use std::ops::Range;

#[derive(Debug, Default, Clone)]
pub struct BlockSelection {
    owner: Option<String>,
    anchor: usize,
    head: usize,
    dragging: bool,
    text: String,
}

impl BlockSelection {
    /// The selected bytes of block `owner`, when it is the selected one and something is selected.
    pub fn range_of(&self, owner: &str) -> Option<Range<usize>> {
        (self.owner.as_deref() == Some(owner) && self.anchor != self.head)
            .then(|| self.anchor.min(self.head)..self.anchor.max(self.head))
    }

    /// The selected text, for the clipboard.
    pub fn text(&self) -> Option<&str> {
        (!self.text.is_empty()).then_some(self.text.as_str())
    }

    /// Whether the pointer is down and extending the selection in block `owner`.
    pub fn is_dragging_in(&self, owner: &str) -> bool {
        self.dragging && self.owner.as_deref() == Some(owner)
    }

    /// The pointer went down at byte `at` of `content`, which `owner` holds: `clicks` is how many
    /// presses in a row. One starts a drag; two select the word; three or more select the line.
    pub fn press(&mut self, owner: &str, content: &str, at: usize, clicks: usize) {
        let at = floor_boundary(content, at);
        self.owner = Some(owner.to_owned());
        self.dragging = clicks <= 1;
        let (anchor, head) = match clicks {
            0 | 1 => (at, at),
            2 => {
                let word = word_range(content, at);
                (word.start, word.end)
            }
            _ => {
                let line = line_range(content, at);
                (line.start, line.end)
            }
        };
        self.anchor = anchor;
        self.head = head;
        self.keep(content);
    }

    /// The pointer moved to byte `at` of `content`, which `owner` holds. False unless the drag is
    /// in that block.
    pub fn drag(&mut self, owner: &str, content: &str, at: usize) -> bool {
        if !self.is_dragging_in(owner) {
            return false;
        }
        self.head = floor_boundary(content, at);
        self.keep(content);
        true
    }

    pub fn release(&mut self) {
        self.dragging = false;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn keep(&mut self, content: &str) {
        let range = self.anchor.min(self.head)..self.anchor.max(self.head);
        self.text = content.get(range).unwrap_or_default().to_owned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_selects_from_where_it_began_in_either_direction() {
        let mut selection = BlockSelection::default();
        let content = "hello world";
        selection.press("a", content, 6, 1);
        assert_eq!(selection.range_of("a"), None);
        assert!(selection.drag("a", content, 9));
        assert_eq!(selection.range_of("a"), Some(6..9));
        assert_eq!(selection.text(), Some("wor"));
        assert!(selection.drag("a", content, 2));
        assert_eq!(selection.range_of("a"), Some(2..6));
        assert_eq!(selection.text(), Some("llo "));
        assert!(
            !selection.drag("b", content, 0),
            "another block's drag is not this one's"
        );
        selection.release();
        assert!(!selection.drag("a", content, 0));
        assert_eq!(selection.text(), Some("llo "));
    }

    #[test]
    fn only_the_block_that_was_chosen_shows_a_selection() {
        let mut selection = BlockSelection::default();
        selection.press("a", "one two", 0, 2);
        assert_eq!(selection.range_of("a"), Some(0..3));
        assert_eq!(selection.range_of("b"), None);
        selection.press("b", "three", 1, 1);
        assert_eq!(selection.range_of("a"), None);
        assert_eq!(selection.text(), None);
    }

    #[test]
    fn two_clicks_take_a_word_and_three_a_line() {
        let mut selection = BlockSelection::default();
        let content = "cargo test\nsecond line";
        selection.press("a", content, 7, 2);
        assert_eq!(selection.text(), Some("test"));
        assert!(!selection.is_dragging_in("a"));
        selection.press("a", content, 14, 3);
        assert_eq!(selection.text(), Some("second line"));
    }

    #[test]
    fn a_point_past_the_end_or_inside_a_character_is_rounded() {
        let mut selection = BlockSelection::default();
        let content = "\u{e9}a";
        selection.press("a", content, 99, 1);
        selection.drag("a", content, 1);
        assert_eq!(selection.text(), Some("\u{e9}a"));
    }

    #[test]
    fn clearing_forgets_everything() {
        let mut selection = BlockSelection::default();
        selection.press("a", "word", 0, 2);
        selection.clear();
        assert_eq!(selection.range_of("a"), None);
        assert_eq!(selection.text(), None);
    }
}

//! The text of an agent composer: what the person has typed, where the caret is, and the part
//! an input method is still composing. GPUI has no text field, so the composer edits this value
//! itself, from the platform's text input (dead keys, input methods, emoji) and from editing keys.
//! The platform speaks in UTF-16 offsets; the draft keeps byte offsets and converts.

use std::ops::Range;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Draft {
    text: String,
    /// Byte offset of the caret; always on a character boundary.
    caret: usize,
    /// The bytes an input method is composing (shown underlined by the platform), if any.
    marked: Option<Range<usize>>,
}

impl Draft {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn is_composing(&self) -> bool {
        self.marked.is_some()
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.caret, text);
        self.caret += text.len();
    }

    pub fn backspace(&mut self) {
        if let Some(start) = self.previous(self.caret) {
            self.text.replace_range(start..self.caret, "");
            self.caret = start;
        }
    }

    pub fn delete(&mut self) {
        if let Some(end) = self.next(self.caret) {
            self.text.replace_range(self.caret..end, "");
        }
    }

    pub fn left(&mut self) {
        if let Some(start) = self.previous(self.caret) {
            self.caret = start;
        }
    }

    pub fn right(&mut self) {
        if let Some(end) = self.next(self.caret) {
            self.caret = end;
        }
    }

    /// The start of the caret's line.
    pub fn home(&mut self) {
        self.caret = self.text[..self.caret].rfind('\n').map_or(0, |at| at + 1);
    }

    /// The end of the caret's line.
    pub fn end(&mut self) {
        self.caret = self.text[self.caret..]
            .find('\n')
            .map_or(self.text.len(), |at| self.caret + at);
    }

    /// The text to send, trimmed; `None` when there is nothing but space.
    pub fn to_send(&self) -> Option<String> {
        let text = self.text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The platform's offsets, in UTF-16 units, as byte offsets.
    pub fn bytes_of(&self, range: &Range<usize>) -> Range<usize> {
        self.byte_offset(range.start)..self.byte_offset(range.end)
    }

    pub fn units_of(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_offset(range.start)..self.utf16_offset(range.end)
    }

    pub fn caret_utf16(&self) -> usize {
        self.utf16_offset(self.caret)
    }

    pub fn marked_bytes(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    pub fn marked_utf16(&self) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.units_of(range))
    }

    pub fn unmark(&mut self) {
        self.marked = None;
    }

    /// Replaces `range` (UTF-16; the composing text, else the caret, when `None`) with final text.
    pub fn replace_in_range(&mut self, range: Option<Range<usize>>, text: &str) {
        let range = self.replaced(range);
        self.text.replace_range(range.clone(), text);
        self.caret = range.start + text.len();
        self.marked = None;
    }

    /// Replaces `range` with text an input method is still composing. `selected` is the caret
    /// or selection inside that text, in UTF-16 units relative to its start.
    pub fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
    ) {
        let range = self.replaced(range);
        self.text.replace_range(range.clone(), text);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        let inside = selected.map_or(text.len(), |selected| {
            let mut offsets = text.encode_utf16().count();
            offsets = selected.end.min(offsets);
            Self::bytes_of_utf16(text, offsets)
        });
        self.caret = range.start + inside;
    }

    fn replaced(&self, range: Option<Range<usize>>) -> Range<usize> {
        range
            .map(|range| self.bytes_of(&range))
            .or_else(|| self.marked.clone())
            .unwrap_or(self.caret..self.caret)
    }

    fn byte_offset(&self, utf16: usize) -> usize {
        Self::bytes_of_utf16(&self.text, utf16)
    }

    fn bytes_of_utf16(text: &str, utf16: usize) -> usize {
        let mut seen = 0;
        for (at, character) in text.char_indices() {
            if seen >= utf16 {
                return at;
            }
            seen += character.len_utf16();
        }
        text.len()
    }

    fn utf16_offset(&self, byte: usize) -> usize {
        self.text[..byte.min(self.text.len())]
            .encode_utf16()
            .count()
    }

    fn previous(&self, from: usize) -> Option<usize> {
        self.text[..from]
            .char_indices()
            .next_back()
            .map(|(at, _)| at)
    }

    fn next(&self, from: usize) -> Option<usize> {
        self.text[from..]
            .chars()
            .next()
            .map(|c| from + c.len_utf8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Draft {
        let mut draft = Draft::default();
        draft.insert(text);
        draft
    }

    #[test]
    fn typing_inserts_at_the_caret() {
        let mut draft = typed("helo");
        draft.left();
        draft.insert("l");
        assert_eq!(draft.text(), "hello");
        assert_eq!(draft.caret(), 4);
    }

    #[test]
    fn backspace_and_delete_remove_whole_characters() {
        let mut draft = typed("a\u{e9}\u{1f44b}");
        draft.backspace();
        assert_eq!(draft.text(), "a\u{e9}");
        draft.left();
        draft.delete();
        assert_eq!(draft.text(), "a");
        draft.backspace();
        draft.backspace();
        assert_eq!(draft.text(), "");
        assert_eq!(draft.caret(), 0);
    }

    #[test]
    fn the_caret_stays_inside_the_text() {
        let mut draft = typed("ab");
        draft.right();
        assert_eq!(draft.caret(), 2);
        draft.home();
        draft.left();
        assert_eq!(draft.caret(), 0);
    }

    #[test]
    fn home_and_end_follow_the_current_line() {
        let mut draft = typed("one\ntwo\nthree");
        draft.left();
        draft.home();
        assert_eq!(draft.caret(), 8);
        draft.end();
        assert_eq!(draft.caret(), 13);
        draft.insert("!");
        draft.home();
        draft.left();
        draft.end();
        assert_eq!(draft.caret(), 7);
    }

    #[test]
    fn what_is_sent_is_trimmed_and_blank_text_is_not_sent() {
        let draft = typed("  oi \n");
        assert_eq!(draft.to_send().as_deref(), Some("oi"));
        assert_eq!(typed(" \n").to_send(), None);
        let mut cleared = draft;
        cleared.clear();
        assert!(cleared.is_blank());
        assert_eq!(cleared.caret(), 0);
    }

    #[test]
    fn offsets_convert_between_utf16_and_bytes() {
        // "a", "c with cedilla" (2 bytes, 1 unit), "tilde a" (2 bytes), "o", an emoji (4 bytes, 2 units)
        let draft = typed("a\u{e7}\u{e3}o\u{1f44b}");
        assert_eq!(draft.bytes_of(&(1..3)), 1..5);
        assert_eq!(draft.units_of(&(5..10)), 3..6);
        assert_eq!(draft.caret_utf16(), 6);
        assert_eq!(draft.bytes_of(&(0..99)), 0..10);
    }

    #[test]
    fn a_dead_key_composes_in_place_and_commits_without_extra_text() {
        let mut draft = typed("a");
        // The platform marks "~" while the dead key waits, then replaces it with the letter.
        draft.replace_and_mark(None, "~", None);
        assert_eq!(draft.text(), "a~");
        assert_eq!(draft.marked_utf16(), Some(1..2));
        assert!(draft.is_composing());
        draft.replace_in_range(None, "\u{e3}");
        assert_eq!(draft.text(), "a\u{e3}");
        assert!(!draft.is_composing());
        draft.replace_in_range(None, "o");
        assert_eq!(draft.text(), "a\u{e3}o");
        assert_eq!(draft.caret(), 4);
    }

    #[test]
    fn typing_replaces_an_explicit_utf16_range() {
        let mut draft = typed("ao");
        draft.replace_in_range(Some(1..2), "\u{e7}\u{e3}o");
        assert_eq!(draft.text(), "a\u{e7}\u{e3}o");
        draft.unmark();
        assert_eq!(draft.marked_utf16(), None);
    }

    #[test]
    fn the_caret_inside_composing_text_follows_the_selection() {
        let mut draft = Draft::default();
        draft.replace_and_mark(None, "\u{3053}\u{3093}", Some(1..1));
        assert_eq!(draft.caret(), 3);
    }
}

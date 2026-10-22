//! The text of a one- or many-line input (the agent composer, a name, a search): what is
//! written, the selection (a caret is an empty one), the part an input method is still
//! composing, and the steps that undo and redo walk. It is a plain value with no window in
//! it, so every rule of editing is tested here; the view only draws it and forwards keys, the
//! pointer and the platform's text input.
//!
//! The platform speaks in UTF-16 offsets; the input keeps byte offsets and converts. The caret
//! moves over a grapheme cluster and by words as the editor pane does (`editor::text`).

pub mod block;
mod history;
pub mod keys;
#[cfg(test)]
mod tests;

use crate::editor::{
    selection::Motion,
    text::{
        byte_at, cells_before, cluster_end, cluster_start, column_at, column_at_cell, word_after,
        word_at, word_before,
    },
};
use history::{History, Kind, Snapshot};
use std::{borrow::Cow, ops::Range};

#[derive(Debug, Default, Clone)]
pub struct TextInput {
    text: String,
    /// Byte offset where the selection started; always on a character boundary.
    anchor: usize,
    /// Byte offset of the caret, the end of the selection that moves.
    head: usize,
    /// The bytes an input method is composing (shown underlined by the platform), if any.
    marked: Option<Range<usize>>,
    /// The cell the caret keeps while it moves up and down through lines of different lengths.
    desired: Option<usize>,
    history: History,
}

/// Line breaks as one `\n`: pasted text may bring `\r\n` or a lone `\r`.
fn normalized(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// `at` clamped to the text and rounded down to a character boundary.
pub fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The bytes of the line `at` is on, without its line break. `at` is on a character boundary.
pub fn line_range(text: &str, at: usize) -> Range<usize> {
    let start = text[..at].rfind('\n').map_or(0, |found| found + 1);
    let end = text[at..].find('\n').map_or(text.len(), |found| at + found);
    start..end
}

/// The bytes of the word under `at`, as a double click selects it (the run of example or of
/// punctuation when it is not on a word), within its line. `at` is on a character boundary.
pub fn word_range(text: &str, at: usize) -> Range<usize> {
    let line = line_range(text, at);
    let within = &text[line.clone()];
    let (start, end) = word_at(within, column_at(within, at - line.start));
    line.start + byte_at(within, start)..line.start + byte_at(within, end)
}

impl TextInput {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret: the end of the selection that moves.
    pub fn caret(&self) -> usize {
        self.head
    }

    /// The selected bytes, empty when there is only a caret.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.head
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.has_selection().then(|| &self.text[self.selection()])
    }

    pub fn is_composing(&self) -> bool {
        self.marked.is_some()
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// The text to send, trimmed; `None` when there is nothing but space.
    pub fn to_send(&self) -> Option<String> {
        let text = self.text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            anchor: self.anchor,
            head: self.head,
        }
    }

    fn restore(&mut self, state: Snapshot) {
        self.text = state.text;
        self.anchor = state.anchor;
        self.head = state.head;
        self.marked = None;
        self.desired = None;
    }

    /// Replaces `range` with `text` and puts the caret after it. With `kind` the edit is
    /// recorded for undo.
    fn splice(&mut self, range: Range<usize>, text: &str, kind: Option<Kind>) {
        if let Some(kind) = kind {
            let collapsed = !self.has_selection() && !self.is_composing();
            let (text, anchor, head) = (&self.text, self.anchor, self.head);
            self.history.record(kind, collapsed, || Snapshot {
                text: text.clone(),
                anchor,
                head,
            });
        }
        self.text.replace_range(range.clone(), text);
        self.head = range.start + text.len();
        self.anchor = self.head;
        self.marked = None;
        self.desired = None;
    }

    /// Inserts pasted or programmatic text over the selection, as a step of its own.
    pub fn insert(&mut self, text: &str) {
        let range = self.selection();
        self.splice(range, &normalized(text), Some(Kind::Other));
    }

    /// Deletes the selection and returns it, for a cut.
    pub fn cut(&mut self) -> Option<String> {
        let range = self.selection();
        let cut = self.selected_text()?.to_owned();
        self.splice(range, "", Some(Kind::Other));
        Some(cut)
    }

    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.insert("");
            return;
        }
        if self.head > 0 {
            let start = cluster_start(&self.text, self.head);
            self.splice(start..self.head, "", Some(Kind::Back));
        }
    }

    pub fn delete(&mut self) {
        if self.has_selection() {
            self.insert("");
            return;
        }
        if self.head < self.text.len() {
            let end = cluster_end(&self.text, self.head);
            self.splice(self.head..end, "", Some(Kind::Forward));
        }
    }

    /// Deletes the selection, else the word before or after the caret.
    pub fn delete_word(&mut self, backward: bool) {
        if self.has_selection() {
            self.insert("");
            return;
        }
        let to = self.word_target(backward);
        let range = if backward {
            to..self.head
        } else {
            self.head..to
        };
        if !range.is_empty() {
            self.splice(range, "", Some(Kind::Other));
        }
    }

    pub fn undo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(state) = self.history.undo(current) else {
            return false;
        };
        self.restore(state);
        true
    }

    pub fn redo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(state) = self.history.redo(current) else {
            return false;
        };
        self.restore(state);
        true
    }

    /// The byte range of the line `at` is on, without its line break.
    fn line_of(&self, at: usize) -> Range<usize> {
        line_range(&self.text, at)
    }

    /// Where a word motion from the caret ends. A line break is a stop of its own.
    fn word_target(&self, backward: bool) -> usize {
        let line = self.line_of(self.head);
        let text = &self.text[line.clone()];
        let column = column_at(text, self.head - line.start);
        if backward {
            if self.head == line.start {
                self.head.saturating_sub(1)
            } else {
                line.start + byte_at(text, word_before(text, column))
            }
        } else if self.head == line.end {
            (self.head + 1).min(self.text.len())
        } else {
            line.start + byte_at(text, word_after(text, column))
        }
    }

    /// Moves the caret; with `extend` the selection grows from where it started, else it
    /// collapses (to the side the motion goes, for a left or right arrow).
    pub fn move_by(&mut self, motion: Motion, extend: bool) {
        self.history.close();
        let selection = self.selection();
        let collapse = !extend && self.has_selection();
        let to = match motion {
            Motion::Left if collapse => selection.start,
            Motion::Right if collapse => selection.end,
            Motion::Left => cluster_start(&self.text, self.head),
            Motion::Right => cluster_end(&self.text, self.head),
            Motion::LineStart => self.line_of(self.head).start,
            Motion::LineEnd => self.line_of(self.head).end,
            Motion::WordLeft => self.word_target(true),
            Motion::WordRight => self.word_target(false),
            Motion::DocumentStart => 0,
            Motion::DocumentEnd => self.text.len(),
            Motion::Up | Motion::Down => self.stepped(self.head, motion == Motion::Up),
            Motion::PageUp(lines) | Motion::PageDown(lines) => {
                let up = matches!(motion, Motion::PageUp(_));
                (0..lines).fold(self.head, |at, _| self.stepped(at, up))
            }
        };
        if !matches!(motion, Motion::Up | Motion::Down) {
            self.desired = None;
        }
        self.head = to;
        if !extend {
            self.anchor = to;
        }
    }

    /// The caret one line up or down, at the cell it keeps (left in `desired`); the start of
    /// the text above the first line, and its end below the last.
    fn stepped(&mut self, from: usize, up: bool) -> usize {
        let line = self.line_of(from);
        let text = &self.text[line.clone()];
        let cell = self
            .desired
            .unwrap_or_else(|| cells_before(text, column_at(text, from - line.start)));
        self.desired = Some(cell);
        let target = if up {
            if line.start == 0 {
                return 0;
            }
            self.line_of(line.start - 1)
        } else {
            if line.end >= self.text.len() {
                return self.text.len();
            }
            self.line_of(line.end + 1)
        };
        let above = &self.text[target.clone()];
        target.start + byte_at(above, column_at_cell(above, cell))
    }

    /// Puts the caret at byte `at` (rounded down to a character boundary), as a click or a
    /// drag does; with `extend` the selection grows from where it started.
    pub fn place(&mut self, at: usize, extend: bool) {
        self.history.close();
        self.desired = None;
        let at = self.boundary(at);
        self.head = at;
        if !extend {
            self.anchor = at;
        }
    }

    fn boundary(&self, at: usize) -> usize {
        floor_boundary(&self.text, at)
    }

    pub fn select_all(&mut self) {
        self.history.close();
        self.desired = None;
        self.anchor = 0;
        self.head = self.text.len();
    }

    /// Selects the word under byte `at`, as a double click does.
    pub fn select_word(&mut self, at: usize) {
        let at = self.boundary(at);
        self.select(word_range(&self.text, at));
    }

    /// Selects the line under byte `at`, as a triple click does.
    pub fn select_line(&mut self, at: usize) {
        let at = self.boundary(at);
        self.select(line_range(&self.text, at));
    }

    fn select(&mut self, range: Range<usize>) {
        self.history.close();
        self.desired = None;
        self.anchor = range.start;
        self.head = range.end;
    }

    /// The platform's offsets, in UTF-16 units, as byte offsets.
    pub fn bytes_of(&self, range: &Range<usize>) -> Range<usize> {
        self.byte_offset(range.start)..self.byte_offset(range.end)
    }

    pub fn units_of(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_offset(range.start)..self.utf16_offset(range.end)
    }

    /// The selection in UTF-16 units, with whether the caret is at its start.
    pub fn selection_utf16(&self) -> (Range<usize>, bool) {
        (self.units_of(&self.selection()), self.head < self.anchor)
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

    /// Replaces `range` (UTF-16; the composing text, else the selection, when `None`) with
    /// final text: what the platform delivers for typing, a committed composition and a pick of
    /// an emoji. Typing joins the step before it.
    pub fn replace_in_range(&mut self, range: Option<Range<usize>>, text: &str) {
        let typing = range.is_none() && !self.is_composing();
        let finishing = self.is_composing();
        let range = self.replaced(range);
        let text = normalized(text);
        let kind = if finishing {
            // The step began when the composition did.
            None
        } else if typing {
            Some(Kind::of_typing(&text))
        } else {
            Some(Kind::Other)
        };
        self.splice(range, &text, kind);
    }

    /// Replaces `range` with text an input method is still composing. `selected` is the caret
    /// or selection inside that text, in UTF-16 units relative to its start.
    pub fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
    ) {
        let starting = !self.is_composing();
        let range = self.replaced(range);
        let kind = starting.then_some(Kind::Other);
        self.splice(range.clone(), text, kind);
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        let inside = selected.map_or(text.len(), |selected| {
            Self::bytes_of_utf16(text, selected.end.min(text.encode_utf16().count()))
        });
        self.head = range.start + inside;
        self.anchor = self.head;
    }

    fn replaced(&self, range: Option<Range<usize>>) -> Range<usize> {
        range
            .map(|range| self.bytes_of(&range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection())
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
}

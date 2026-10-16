//! What the person does while the caret is on its way to a page that is not loaded yet.
//!
//! A jump to the start or the end of a long file lands when the engine has sent the page that
//! holds the target. A key pressed before that must act at the target, not at the caret the jump
//! left, so the action waits, in order, and is done when the page has been installed (the Mac's
//! text view gets the same from its page turn; the case is `contracts/vectors/editor.json`).

use super::EditorFile;
use crate::{
    Effect,
    editor::{
        editing::{self, Plan},
        page::{Page, position},
        selection::{Motion, Selection},
        text,
    },
};
use soyeht_protocol::{TextPosition, TextRange, limits::EDITOR_EDIT_BYTES};

/// One thing the person did to the editor.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// A character, a line break or a tab, typed over the selection.
    Insert(String),
    /// Text pasted as it is (line breaks already made the file's).
    Paste(String),
    Delete {
        backward: bool,
    },
    DeleteWord {
        backward: bool,
    },
    Motion {
        motion: Motion,
        extend: bool,
    },
    SelectAll,
    /// A click or a drag to a position of the document.
    Click {
        at: TextPosition,
        extend: bool,
    },
    /// A double click (a word) or a triple click (`line`).
    SelectAround {
        at: TextPosition,
        line: bool,
    },
    Save,
    Replay {
        redo: bool,
    },
}

/// The most actions that wait for a jump, and (with `EDITOR_EDIT_BYTES`) the most text they
/// insert: a page that never comes does not make the replica grow without end.
const HELD_ACTIONS: usize = 1000;

impl Action {
    /// The bytes of text the action inserts.
    fn inserted_bytes(&self) -> usize {
        match self {
            Self::Insert(text) | Self::Paste(text) => text.len(),
            _ => 0,
        }
    }

    /// Whether `next`, coming straight after `self`, makes it needless: a run of drags to one
    /// side or another, of clicks, or of jumps to the same end of the file leaves the last one
    /// where the whole run would. Motions that depend on where the caret is (`Down`, `WordLeft`)
    /// are never merged: each one counts.
    fn superseded_by(&self, next: &Self) -> bool {
        match (self, next) {
            (Self::Click { extend: a, .. }, Self::Click { extend: b, .. }) => a == b,
            (
                Self::Motion {
                    motion: a,
                    extend: x,
                },
                Self::Motion {
                    motion: b,
                    extend: y,
                },
            ) => a == b && x == y && matches!(a, Motion::DocumentStart | Motion::DocumentEnd),
            (Self::SelectAll, Self::SelectAll) => true,
            _ => false,
        }
    }
}

impl EditorFile {
    /// Does `action`, or keeps it until the caret that is on its way has landed.
    pub fn act(&mut self, action: Action) -> Vec<Effect> {
        if self.is_landing() || !self.held.is_empty() {
            self.hold(action);
            return Vec::new();
        }
        // The notice of dropped keys lasts until the next key that is done.
        self.dropped_keys = 0;
        self.perform(action)
    }

    /// Whether actions wait: for a caret that is on its way, or for the page an undo or redo
    /// leaves.
    pub fn is_landing(&self) -> bool {
        self.pending.is_some() || self.replaying
    }

    fn hold(&mut self, action: Action) {
        if let Some(last) = self.held.back_mut()
            && last.superseded_by(&action)
        {
            *last = action;
            return;
        }
        let held: usize = self.held.iter().map(Action::inserted_bytes).sum();
        if self.held.len() >= HELD_ACTIONS || held + action.inserted_bytes() > EDITOR_EDIT_BYTES {
            // Past the bound the newest input is the one refused; the view says so.
            self.dropped_keys = self.dropped_keys.saturating_add(1);
            if !self.overflowed {
                self.overflowed = true;
                soyeht_platform::log::event("desktop.editor_keys_overflow", &[]);
            }
            return;
        }
        self.held.push_back(action);
    }

    /// Drops what waited for a landing that will not come (the read failed, the write was
    /// refused, the connection was lost). This is deliberate: the actions were made for a place
    /// in the document that is not known any more, and doing them at another would edit the
    /// wrong text. The count is kept for the view and logged, never the text.
    pub(super) fn abandon(&mut self) {
        let dropped = u32::try_from(self.held.len()).unwrap_or(u32::MAX);
        self.held.clear();
        self.overflowed = false;
        if dropped > 0 {
            self.dropped_keys = self.dropped_keys.saturating_add(dropped);
            soyeht_platform::log::event(
                "desktop.editor_keys_dropped",
                &[("count", &dropped.to_string())],
            );
        }
    }

    /// Does what waited, in order, until the caret is on its way again (or an undo is) or nothing
    /// waits.
    pub(super) fn release(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        let mut released = false;
        while !self.is_landing() {
            let Some(action) = self.held.pop_front() else {
                break;
            };
            released = true;
            effects.extend(self.perform(action));
        }
        if released && self.held.is_empty() {
            self.overflowed = false;
            // A close that waited for these keys goes on once they have reached the engine: now
            // when they sent nothing, else when the engine has taken the writes.
            if !self.has_unsent() {
                effects.extend(self.settled());
            }
        }
        effects
    }

    fn perform(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Insert(text) => {
                self.change(|page, ranges| editing::insertion(page, ranges, &text))
            }
            Action::Paste(text) => {
                let strings = [text];
                self.change(|page, ranges| editing::verbatim(page, ranges, &strings))
            }
            Action::Delete { backward } => {
                self.change(|page, ranges| editing::deletion(page, ranges, backward))
            }
            Action::DeleteWord { backward } => {
                self.change(|page, ranges| delete_word(page, ranges, backward))
            }
            Action::Motion { motion, extend } => self.motion(motion, extend),
            Action::SelectAll => self.select_all(),
            Action::Click { at, extend } => {
                let target = self.selection.moved_to(at, extend);
                self.place(target)
            }
            Action::SelectAround { at, line } => self.select_around(&at, line),
            Action::Save => self.save(),
            Action::Replay { redo } => self.replay(redo),
        }
    }

    /// Makes the plan `plan` draws up for the selection.
    fn change(&mut self, plan: impl FnOnce(&Page, &[TextRange]) -> Option<Plan>) -> Vec<Effect> {
        let Some(range) = self.range_to_edit() else {
            return Vec::new();
        };
        let made = self.page.as_ref().and_then(|page| plan(page, &[range]));
        made.map(|made| self.edit(&made)).unwrap_or_default()
    }

    fn select_around(&mut self, at: &TextPosition, line: bool) -> Vec<Effect> {
        let Some(page) = self.page.as_ref().filter(|page| page.holds(at.line)) else {
            return Vec::new();
        };
        let (start, end) = if line {
            (
                position(at.line, 0),
                position(at.line, page.line_end(at.line)),
            )
        } else {
            let (from, to) = text::word_at(page.text(at.line), at.column);
            (position(at.line, from), position(at.line, to))
        };
        self.place(Selection::selecting(start, end))
    }
}

/// Backspace or Delete over the word before or after the caret; over a selection, or at the edge
/// of a line, it is the plain key.
fn delete_word(page: &Page, ranges: &[TextRange], backward: bool) -> Option<Plan> {
    let [range] = ranges else {
        return editing::deletion(page, ranges, backward);
    };
    let at = &range.start;
    let line = page.text(at.line);
    let at_edge = if backward {
        at.column == 0
    } else {
        at.column >= page.line_end(at.line)
    };
    if *at != range.end || at_edge {
        return editing::deletion(page, ranges, backward);
    }
    let edge = if backward {
        text::word_before(line, at.column)
    } else {
        text::word_after(line, at.column)
    };
    let (from, to) = (edge.min(at.column), edge.max(at.column));
    let span = TextRange {
        start: position(at.line, from),
        end: position(at.line, to),
    };
    editing::deletion(page, &[span], backward)
}

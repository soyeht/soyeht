//! The undo and redo stacks of a text input, and when an edit joins the one before it.
//!
//! A run of typing is one step, and so is a run of Backspace or of Delete, as in any text field.
//! Typing breaks into a new step where a new word starts after a space, and a step ends the
//! moment the caret is moved or the text is changed in any other way (a paste, a cut, a newline,
//! a selection that was replaced).

/// What an edit was, for deciding whether it joins the step before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// Letters and the like typed at the caret.
    Word,
    /// Example typed at the caret.
    Space,
    Back,
    Forward,
    /// Anything else: always a step of its own.
    Other,
}

impl Kind {
    /// What typing `text` is.
    pub(super) fn of_typing(text: &str) -> Self {
        if text.contains('\n') {
            Self::Other
        } else if !text.is_empty() && text.chars().all(char::is_whitespace) {
            Self::Space
        } else {
            Self::Word
        }
    }

    /// Whether an edit of kind `self` continues the open step of kind `open`.
    fn joins(self, open: Self) -> bool {
        matches!(
            (open, self),
            (Self::Word, Self::Word | Self::Space)
                | (Self::Space, Self::Space)
                | (Self::Back, Self::Back)
                | (Self::Forward, Self::Forward)
        )
    }
}

/// The text with its selection, as it was before an edit.
#[derive(Debug, Clone)]
pub(super) struct Snapshot {
    pub(super) text: String,
    pub(super) anchor: usize,
    pub(super) head: usize,
}

/// How many steps are kept.
const DEPTH: usize = 200;

#[derive(Debug, Default, Clone)]
pub(super) struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The kind of the step that a next edit may continue.
    open: Option<Kind>,
}

impl History {
    /// Called before an edit that changes the text. `before` is the state the edit starts from;
    /// it is kept unless the edit continues the open step.
    pub(super) fn record(
        &mut self,
        kind: Kind,
        collapsed: bool,
        before: impl FnOnce() -> Snapshot,
    ) {
        self.redo.clear();
        let continues = collapsed && self.open.is_some_and(|open| kind.joins(open));
        if !continues {
            if self.undo.len() == DEPTH {
                self.undo.remove(0);
            }
            self.undo.push(before());
        }
        self.open = (kind != Kind::Other).then_some(kind);
    }

    /// The caret moved: the next edit starts a step.
    pub(super) fn close(&mut self) {
        self.open = None;
    }

    /// The state to go back to, with `current` kept for redo.
    pub(super) fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.open = None;
        Some(previous)
    }

    pub(super) fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.open = None;
        Some(next)
    }
}

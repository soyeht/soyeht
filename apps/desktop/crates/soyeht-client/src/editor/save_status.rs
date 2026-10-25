//! Whether the engine could save a file, and why not, as the Mac's `EditorSaveStatus`. The
//! engine tells it (`file.save_failed`, and `saveFailure` on the document it reads), so the
//! pane shows what the engine found and never a guess of its own. The cases are
//! `contracts/vectors/editor.json` (`saveStatus`), which both clients pass. The editor pane and
//! the text, screenplay and presentation panes show it through one banner (`views/editor.rs`).

use soyeht_protocol::{
    EditorSaveFailure, FileReadResult,
    client_policy::{EDITOR_SAVE_DENIED, EDITOR_SAVE_FULL, EDITOR_SAVE_MISSING, EDITOR_SAVE_OTHER},
};

/// Why the last save of a file failed, until it is saved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SaveStatus {
    failure: Option<EditorSaveFailure>,
}

impl SaveStatus {
    /// The engine told that a save failed. Edits made meanwhile do not end the failure: only
    /// a save does, or the engine's word on a read.
    pub fn told(&mut self, reason: EditorSaveFailure) {
        self.failure = Some(reason);
    }

    /// The engine saved the file.
    pub fn saved(&mut self) {
        self.failure = None;
    }

    /// A page the engine read: it says whether the last save failed, and a file with nothing
    /// to save has no failure.
    pub fn read(&mut self, page: &FileReadResult) {
        self.failure = page.save_failure.clone().filter(|_| page.file.dirty);
    }

    pub fn failure(&self) -> Option<&EditorSaveFailure> {
        self.failure.as_ref()
    }

    /// What the pane says about the failure, if there is one.
    pub fn text(&self) -> Option<&'static str> {
        self.failure.as_ref().map(text_of)
    }
}

/// What the pane says of a failure: why the file could not be saved and what to do.
pub fn text_of(reason: &EditorSaveFailure) -> &'static str {
    match reason {
        EditorSaveFailure::Missing => EDITOR_SAVE_MISSING,
        EditorSaveFailure::Denied => EDITOR_SAVE_DENIED,
        EditorSaveFailure::Full => EDITOR_SAVE_FULL,
        EditorSaveFailure::Other => EDITOR_SAVE_OTHER,
    }
}

//! A file shown in a text field (a writing pane): the field holds the whole document as one
//! string, and what the person typed reaches the page, and the engine, as the one edit that turns
//! the page's text into the field's (`writing::document`). The edits go the way every edit does:
//! into the page at once, and to the engine in order, each to the revision the one before made.

use super::{Deferred, EditorFile};
use crate::{
    Command, Effect,
    editor::{
        editing::{Plan, Replacement},
        page::{CRLF, LF},
        selection::Selection,
    },
    writing::{Command as WritingCommand, document},
};
use soyeht_protocol::{TextEdit, TextRange, WritingFormatKind, limits::EDITOR_EDIT_BYTES};

impl EditorFile {
    /// Why a text that is not the whole document cannot be edited as a string.
    pub const PARTIAL: &'static str = "This text is longer than the desktop client can edit yet.";
    /// Why a text with a lone `\r` in a line cannot be edited as a string.
    pub const LONE_CR: &'static str =
        "This text has line breaks the desktop client cannot edit yet.";
    /// Why an edit the engine would refuse for its size is refused here.
    pub const TOO_LARGE: &'static str = "That change is too large to make at once.";

    /// Why this file's text cannot be edited as a string, when it cannot: the page is not the
    /// whole document, or a line holds a lone `\r`, which the field would read as a line break
    /// and the page as a character.
    pub fn why_not_editable(&self) -> Option<&'static str> {
        let page = self.page.as_ref()?;
        if page.from_line != 0 || page.to_line() < self.lines {
            return Some(Self::PARTIAL);
        }
        page.lines
            .iter()
            .any(|line| line.text.contains('\r'))
            .then_some(Self::LONE_CR)
    }

    /// The document as a string, when it can be edited as one.
    pub fn whole_text(&self) -> Option<String> {
        let page = self
            .page
            .as_ref()
            .filter(|_| self.why_not_editable().is_none())?;
        Some(document::text_of(page))
    }

    /// The person typed in a field that showed this file's text: `after` is the text now, and
    /// `selection` the bytes it has selected (anchor and head). A text that is the page's already
    /// asks nothing of the engine and changes nothing.
    pub fn retype(
        &mut self,
        after: &str,
        selection: (usize, usize),
    ) -> Result<Vec<Effect>, &'static str> {
        if let Some(reason) = self.why_not_editable() {
            return Err(reason);
        }
        let Some(page) = self.page.as_ref() else {
            return Err(Self::PARTIAL);
        };
        let before = document::text_of(page);
        let Some(change) = document::change_between(&before, after, 0) else {
            return Ok(Vec::new());
        };
        // A document that breaks its lines with `\r\n` takes the new lines the same way.
        let ending = page.ending(change.range.start.line);
        let text = if ending == CRLF {
            change.text.replace(LF, CRLF)
        } else {
            change.text
        };
        if text.len() > EDITOR_EDIT_BYTES {
            return Err(Self::TOO_LARGE);
        }
        let caret = document::position_of(after, 0, selection.1);
        let plan = Plan {
            replacements: vec![Replacement {
                range: change.range.clone(),
                text: text.clone(),
            }],
            carets: vec![caret],
            edits: vec![TextEdit {
                range: change.range,
                text,
            }],
        };
        let effects = self.edit(&plan);
        self.selection = Selection::selecting(
            document::position_of(after, 0, selection.0),
            document::position_of(after, 0, selection.1),
        );
        Ok(effects)
    }

    /// Formats the part of the text the engine is told of, once the edits before it have arrived.
    pub fn format(&mut self, range: TextRange, kind: WritingFormatKind) -> Vec<Effect> {
        if self.has_unsent() {
            self.defer(Deferred::Format { range, kind });
            return Vec::new();
        }
        vec![Effect::Send(Command::Writing(WritingCommand::Format {
            file_id: self.doc.file_id.clone(),
            base: self.doc.revision.clone(),
            range,
            kind,
        }))]
    }
}

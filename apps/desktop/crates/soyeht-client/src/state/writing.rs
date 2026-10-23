//! The part of the replica that is the writing panes': a document opens in a pane whose text is
//! a file of the editor's replica, the lists of the sections, and what the person does in a text.
//! The rules are in `crate::writing`; this is where they meet the panes and the editor's files.

use super::{Effect, State};
use crate::{
    editor::EditorEvent,
    writing::{Command, WritingEvent, document, rules},
};
use serde_json::Value;
use soyeht_protocol::{
    FileDocument, FileOpenResult, PaneMode, WritingChangedEvent, WritingDocument,
    WritingFormatKind, WritingSection,
};

impl State {
    pub(super) fn writing_event(&mut self, event: WritingEvent) -> Vec<Effect> {
        match event {
            WritingEvent::Opened(opened) => {
                let opened = *opened;
                let mut effects = self.upsert_pane(opened.pane.clone());
                effects.push(Effect::FocusPane(opened.pane.pane_id.clone()));
                let file = FileOpenResult {
                    file: opened.document.file.clone(),
                    pane: opened.pane,
                    viewport: opened.viewport,
                };
                effects.extend(
                    self.editors
                        .apply(EditorEvent::Opened(Box::new(file)), &self.panes),
                );
                self.writings.insert(opened.document);
                effects
            }
            WritingEvent::Formatted(file) => self.editors.file_changed(file),
            other => {
                self.writings.apply(other);
                Vec::new()
            }
        }
    }

    /// A pane was created with its document, by another client or by the CLI: the file's first
    /// page is read.
    pub(super) fn document_created(
        &mut self,
        file: Option<FileDocument>,
        document: Option<WritingDocument>,
    ) -> Vec<Effect> {
        let (Some(file), Some(document)) = (file, document) else {
            return Vec::new();
        };
        self.writings.insert(document);
        self.editors.adopt_file(file)
    }

    /// The engine changed a document (a format, a change an agent made): its text is read again
    /// when this replica is behind.
    pub(super) fn writing_changed(&mut self, params: Value) -> serde_json::Result<Vec<Effect>> {
        let event: WritingChangedEvent = serde_json::from_value(params)?;
        Ok(self.editors.changed_to(&event.file_id, &event.revision))
    }

    /// Asks for the first page of a section's documents, or the next page when `more`.
    pub fn writing_list(
        &mut self,
        workspace_id: &str,
        section: &WritingSection,
        more: bool,
    ) -> Vec<Effect> {
        self.writings.list(workspace_id, section, more)
    }

    /// Makes the document the new-document form describes; nothing is asked while its title is
    /// blank or a document is already being made.
    pub fn writing_create(
        &mut self,
        workspace_id: &str,
        section: &WritingSection,
        draft: &rules::DocumentDraft,
    ) -> Vec<Effect> {
        let creating = self
            .writings
            .section(workspace_id, section)
            .is_some_and(|list| list.creating);
        match draft.submission(creating) {
            Some(submission) => self.writings.create(workspace_id, section, submission),
            None => Vec::new(),
        }
    }

    /// Opens the document at `path` of a section in a pane.
    pub fn writing_open(
        &mut self,
        workspace_id: &str,
        section: &WritingSection,
        path: &str,
    ) -> Vec<Effect> {
        vec![Effect::Send(crate::Command::Writing(Command::Open {
            workspace_id: workspace_id.to_owned(),
            path: path.to_owned(),
            kind: rules::kind(section),
        }))]
    }

    /// What is due as time passes: the words of the documents whose text settled.
    pub fn writing_tick(&mut self) -> Vec<Effect> {
        self.writings.counts_due(&self.editors)
    }

    /// The person typed in the text field of a writing pane: `text` is what it holds and
    /// `selection` the bytes it has selected (anchor and head).
    pub fn writing_retype(
        &mut self,
        pane_id: &str,
        text: &str,
        selection: (usize, usize),
    ) -> Result<Vec<Effect>, &'static str> {
        match self.editors.file_of_pane_mut(pane_id) {
            Some(file) => file.retype(text, selection),
            None => Ok(Vec::new()),
        }
    }

    /// Formats the selected text of a writing pane, which the field holds as `text`.
    pub fn writing_format(
        &mut self,
        pane_id: &str,
        kind: WritingFormatKind,
        text: &str,
        selection: (usize, usize),
    ) -> Vec<Effect> {
        if selection.0 == selection.1 {
            return Vec::new();
        }
        let range = document::range_of(text, 0, selection.0, selection.1);
        self.editors
            .file_of_pane_mut(pane_id)
            .map(|file| file.format(range, kind))
            .unwrap_or_default()
    }

    /// The agent to open beside a document: the one the person prefers when it can start, else
    /// the one that was ready most recently, else any that can start.
    pub fn default_harness(&self) -> Option<String> {
        let preferred = self.preferences().preferred_harness.as_deref();
        crate::harness_picker::default_harness(self.harnesses(), None, preferred)
            .or_else(|| self.harnesses().iter().find(|harness| harness.available))
            .map(|harness| harness.id.clone())
    }

    /// The agent that is the companion of a document, when it has one.
    pub fn companion_of(&self, pane_id: &str) -> Option<String> {
        let workspace_id = self.pane(pane_id)?.workspace_id.clone();
        let tree = self.layout(&workspace_id, |_| true)?;
        crate::layout::companion_of(&tree, pane_id).map(|companion| companion.agent_pane_id.clone())
    }

    /// Whether an agent is being opened beside the pane: the engine has not answered yet, and a
    /// second request would open a second agent.
    pub fn companion_pending(&self, pane_id: &str) -> bool {
        self.expected.iter().any(|expected| {
            expected
                .split
                .as_ref()
                .is_some_and(|(anchor, _)| anchor == pane_id)
        })
    }

    /// Closes a writing pane once its edits have reached the engine, which keeps the draft.
    pub(super) fn writing_close(&mut self, pane_id: &str) -> Vec<Effect> {
        match self.editors.file_of_pane_mut(pane_id) {
            Some(file) => file.close(),
            None => vec![Effect::Send(crate::Command::ClosePane {
                pane_id: pane_id.to_owned(),
            })],
        }
    }

    /// Whether the pane is a writing pane, which closes after its edits have reached the engine.
    pub(super) fn is_writing(&self, pane_id: &str) -> bool {
        self.pane(pane_id)
            .is_some_and(|pane| pane.mode == PaneMode::Writing)
    }
}

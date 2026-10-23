//! The client's replica of the writing documents: which documents the engine keeps in each
//! section of a workspace, and what is known of the ones open in panes. A document's text is a
//! file like any other, so it lives in the editor's replica (`editor`); what is kept here is what
//! only a writing document has: its kind and look, its word count, and the lists of the sections.
//!
//! The rules both clients share are `rules`, whose cases are `contracts/vectors/writing.json`;
//! `document` maps a text field's string to the page of lines the engine is edited through.

pub mod commands;
pub mod document;
pub mod looks;
pub mod rules;

pub use commands::{Command, Failure, Opened, WritingEvent};

use crate::{Effect, editor::Editors};
use soyeht_protocol::{
    WritingDocument, WritingSection, WritingSectionEntry, WritingSectionListResult,
};
use std::collections::HashMap;

/// A document open in a pane.
#[derive(Debug, Clone)]
pub struct Document {
    pub doc: WritingDocument,
    /// The words of the document and the revision they were counted at.
    counted: Option<(u32, String)>,
    /// The revision whose count is on its way, or was asked for and failed: it is asked once.
    asked: Option<String>,
}

impl Document {
    fn new(doc: WritingDocument) -> Self {
        Self {
            doc,
            counted: None,
            asked: None,
        }
    }

    /// The words the engine last counted in the document.
    pub fn words(&self) -> Option<u32> {
        self.counted.as_ref().map(|(words, _)| *words)
    }
}

/// The documents of one section of one workspace, a page at a time.
#[derive(Debug, Clone, Default)]
pub struct SectionList {
    pub entries: Vec<WritingSectionEntry>,
    /// Where the next page starts, when there is one.
    pub next: Option<String>,
    /// A page is on its way.
    pub loading: bool,
    /// The first page has arrived.
    pub loaded: bool,
    /// The last page asked for could not be read.
    pub failed: bool,
    /// A document is being made in this section.
    pub creating: bool,
}

#[derive(Default)]
pub struct Writings {
    /// The documents open in panes, by file.
    documents: HashMap<String, Document>,
    sections: Vec<((String, &'static str), SectionList)>,
}

impl Writings {
    /// The document the pane shows.
    pub fn of_pane(&self, pane_id: &str) -> Option<&Document> {
        self.documents
            .values()
            .find(|document| document.doc.file.pane_id == pane_id)
    }

    /// A fresh connection: the documents the engine lists are the open ones. A count that was
    /// made for the revision the engine still has stays.
    pub fn adopt(&mut self, docs: Vec<WritingDocument>) {
        let mut known = std::mem::take(&mut self.documents);
        for doc in docs {
            let mut document = Document::new(doc);
            if let Some(old) = known.remove(&document.doc.file.file_id) {
                document.counted = old
                    .counted
                    .filter(|(_, revision)| *revision == document.doc.file.revision);
            }
            self.documents
                .insert(document.doc.file.file_id.clone(), document);
        }
        // What a list was waiting for was lost with the connection.
        for (_, list) in &mut self.sections {
            list.loading = false;
            list.creating = false;
        }
    }

    /// A document the engine opened in a pane, or told of with the pane's creation.
    pub fn insert(&mut self, doc: WritingDocument) {
        let section = doc.section.clone();
        let workspace_id = doc.file.workspace_id.clone();
        let file_id = doc.file.file_id.clone();
        match self.documents.get_mut(&file_id) {
            Some(known) => known.doc = doc,
            None => {
                self.documents.insert(file_id, Document::new(doc));
            }
        }
        self.list_of(&workspace_id, &section).creating = false;
    }

    /// A pane closed with its document.
    pub fn forget_pane(&mut self, pane_id: &str) {
        self.documents
            .retain(|_, document| document.doc.file.pane_id != pane_id);
    }

    pub fn section(&self, workspace_id: &str, section: &WritingSection) -> Option<&SectionList> {
        let name = rules::name(section);
        self.sections
            .iter()
            .find(|((workspace, key), _)| workspace == workspace_id && *key == name)
            .map(|(_, list)| list)
    }

    fn list_of(&mut self, workspace_id: &str, section: &WritingSection) -> &mut SectionList {
        let name = rules::name(section);
        let at = self
            .sections
            .iter()
            .position(|((workspace, key), _)| workspace == workspace_id && *key == name);
        let at = at.unwrap_or_else(|| {
            self.sections
                .push(((workspace_id.to_owned(), name), SectionList::default()));
            self.sections.len() - 1
        });
        &mut self.sections[at].1
    }

    /// Asks for the first page of a section, or for the next one when `more`.
    pub fn list(
        &mut self,
        workspace_id: &str,
        section: &WritingSection,
        more: bool,
    ) -> Vec<Effect> {
        let list = self.list_of(workspace_id, section);
        let cursor = if more { list.next.clone() } else { None };
        if list.loading || (more && cursor.is_none()) {
            return Vec::new();
        }
        list.loading = true;
        list.failed = false;
        vec![Effect::Send(crate::Command::Writing(Command::List {
            workspace_id: workspace_id.to_owned(),
            section: section.clone(),
            cursor,
        }))]
    }

    /// Asks for a document to be made in a section.
    pub fn create(
        &mut self,
        workspace_id: &str,
        section: &WritingSection,
        submission: rules::Submission,
    ) -> Vec<Effect> {
        let list = self.list_of(workspace_id, section);
        if list.creating {
            return Vec::new();
        }
        list.creating = true;
        vec![Effect::Send(crate::Command::Writing(Command::Create {
            workspace_id: workspace_id.to_owned(),
            section: section.clone(),
            submission,
        }))]
    }

    /// A page of a section arrived.
    fn listed(
        &mut self,
        workspace_id: &str,
        section: &WritingSection,
        cursor: Option<&str>,
        result: WritingSectionListResult,
    ) {
        let list = self.list_of(workspace_id, section);
        list.loading = false;
        list.loaded = true;
        list.failed = false;
        list.next = result.next_cursor;
        // As on the Mac, only the documents of the section's own kind are listed.
        let kind = rules::kind(section);
        let accepted = result
            .entries
            .into_iter()
            .filter(|entry| entry.section == *section && entry.kind == kind);
        if cursor.is_none() {
            list.entries = accepted.collect();
            return;
        }
        for entry in accepted {
            if list.entries.iter().all(|known| known.path != entry.path) {
                list.entries.push(entry);
            }
        }
    }

    /// What the engine told the writing panes, apart from the opening of a document, which the
    /// state carries out (it owns the panes).
    pub(crate) fn apply(&mut self, event: WritingEvent) {
        match event {
            WritingEvent::Listed {
                workspace_id,
                section,
                cursor,
                result,
            } => self.listed(&workspace_id, &section, cursor.as_deref(), result),
            WritingEvent::Structure(structure) => {
                if let Some(document) = self.documents.get_mut(&structure.document.file.file_id) {
                    document.counted = Some((
                        structure.word_count,
                        structure.document.file.revision.clone(),
                    ));
                    document.doc = structure.document;
                }
            }
            WritingEvent::Failed(failure) => self.failed(&failure),
            WritingEvent::Opened(_) | WritingEvent::Formatted(_) => {}
        }
    }

    fn failed(&mut self, failure: &Failure) {
        match failure {
            Failure::List {
                workspace_id,
                section,
            } => {
                let list = self.list_of(workspace_id, section);
                list.loading = false;
                list.failed = true;
            }
            Failure::Create {
                workspace_id,
                section,
            } => self.list_of(workspace_id, section).creating = false,
            // The count of this revision is not asked again; the next revision asks.
            Failure::Structure { .. } => {}
        }
    }

    /// The counts that are due: a document whose text settled at a revision it has no count
    /// for. Edits on their way hold the count back, since the engine would answer for a
    /// revision that is already past.
    pub fn counts_due(&mut self, editors: &Editors) -> Vec<Effect> {
        let mut effects = Vec::new();
        for document in self.documents.values_mut() {
            let Some(file) = editors.file(&document.doc.file.file_id) else {
                continue;
            };
            let revision = &file.doc.revision;
            let counted = document
                .counted
                .as_ref()
                .is_some_and(|(_, counted)| counted == revision);
            if file.has_unsent() || counted || document.asked.as_ref() == Some(revision) {
                continue;
            }
            document.asked = Some(revision.clone());
            effects.push(Effect::Send(crate::Command::Writing(Command::Structure {
                file_id: file.doc.file_id.clone(),
                revision: revision.clone(),
            })));
        }
        effects
    }
}

#[cfg(test)]
mod tests;

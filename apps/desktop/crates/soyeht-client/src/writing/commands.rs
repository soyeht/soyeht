//! What a writing pane asks of the engine, the typed requests that carry it out, and what the
//! engine's answers tell the replica.

use super::rules::Submission;
use crate::{Result, commands::key, event::Event, rpc::Rpc};
use serde_json::Value;
use soyeht_protocol::{
    FileDocument, FileReadResult, Pane, Request, TextRange, WritingDocument,
    WritingDocumentCreateParams, WritingDocumentCreateResult, WritingDocumentOpenParams,
    WritingDocumentOpenResult, WritingFormatKind, WritingFormatParams, WritingFormatResult,
    WritingKind, WritingSection, WritingSectionListParams, WritingSectionListResult,
    WritingStructureParams, WritingStructureResult, client_policy::PAGE_SIZE_WRITING_SECTION,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// A page of the documents of a section; `cursor` is where the last page ended.
    List {
        workspace_id: String,
        section: WritingSection,
        cursor: Option<String>,
    },
    /// Make a document and open it in a pane.
    Create {
        workspace_id: String,
        section: WritingSection,
        submission: Submission,
    },
    /// Open the document at `path` in a pane.
    Open {
        workspace_id: String,
        path: String,
        kind: WritingKind,
    },
    /// The word count and the structure of a document, as of `revision`.
    Structure { file_id: String, revision: String },
    /// Format the selected text of a document, as of `base`.
    Format {
        file_id: String,
        base: String,
        range: TextRange,
        kind: WritingFormatKind,
    },
}

impl Command {
    /// Fetches that nothing waits on: they run apart from the writes that carry the typing.
    pub fn is_background(&self) -> bool {
        matches!(self, Self::List { .. } | Self::Structure { .. })
    }

    /// Whether the failure is told where it happened, not as a problem: the list says it could
    /// not be read, and a word count that is late is only late.
    pub fn failure_is_shown_in_place(&self) -> bool {
        self.is_background()
    }

    /// What the replica learns when this command fails, so that it does not wait for an answer.
    pub fn failure(&self) -> Option<Event> {
        let failure = match self {
            Self::List {
                workspace_id,
                section,
                ..
            } => Failure::List {
                workspace_id: workspace_id.clone(),
                section: section.clone(),
            },
            Self::Create {
                workspace_id,
                section,
                ..
            } => Failure::Create {
                workspace_id: workspace_id.clone(),
                section: section.clone(),
            },
            Self::Structure { file_id, .. } => Failure::Structure {
                file_id: file_id.clone(),
            },
            Self::Format { .. } | Self::Open { .. } => return None,
        };
        Some(Event::Writing(WritingEvent::Failed(failure)))
    }

    fn request(&self) -> Request {
        match self {
            Self::List {
                workspace_id,
                section,
                cursor,
            } => Request::WritingSectionList(WritingSectionListParams {
                workspace_id: workspace_id.clone(),
                section: section.clone(),
                cursor: cursor.clone(),
                limit: PAGE_SIZE_WRITING_SECTION,
            }),
            Self::Create {
                workspace_id,
                section,
                submission,
            } => Request::WritingDocumentCreate(WritingDocumentCreateParams {
                workspace_id: workspace_id.clone(),
                section: section.clone(),
                title: submission.title.clone(),
                relative_folder: submission.relative_folder.clone(),
                idempotency_key: key(),
            }),
            Self::Open {
                workspace_id,
                path,
                kind,
            } => Request::WritingDocumentOpen(WritingDocumentOpenParams {
                workspace_id: workspace_id.clone(),
                path: path.clone(),
                kind: kind.clone(),
                idempotency_key: key(),
            }),
            Self::Structure { file_id, revision } => {
                Request::WritingStructure(WritingStructureParams {
                    file_id: file_id.clone(),
                    revision: revision.clone(),
                })
            }
            Self::Format {
                file_id,
                base,
                range,
                kind,
            } => Request::WritingFormat(WritingFormatParams {
                file_id: file_id.clone(),
                base_revision: base.clone(),
                range: range.clone(),
                kind: kind.clone(),
                value: None,
                image: None,
                idempotency_key: key(),
            }),
        }
    }

    /// What the engine's answer tells the replica.
    fn outcome(&self, result: Value) -> Result<WritingEvent> {
        Ok(match self {
            Self::List {
                workspace_id,
                section,
                cursor,
            } => WritingEvent::Listed {
                workspace_id: workspace_id.clone(),
                section: section.clone(),
                cursor: cursor.clone(),
                result: serde_json::from_value::<WritingSectionListResult>(result)?,
            },
            Self::Create { .. } => {
                let made = serde_json::from_value::<WritingDocumentCreateResult>(result)?;
                WritingEvent::Opened(Box::new(Opened {
                    document: made.document,
                    pane: made.pane,
                    viewport: made.viewport,
                }))
            }
            Self::Open { .. } => {
                let opened = serde_json::from_value::<WritingDocumentOpenResult>(result)?;
                WritingEvent::Opened(Box::new(Opened {
                    document: opened.document,
                    pane: opened.pane,
                    viewport: opened.viewport,
                }))
            }
            Self::Structure { .. } => WritingEvent::Structure(Box::new(serde_json::from_value::<
                WritingStructureResult,
            >(result)?)),
            Self::Format { .. } => {
                WritingEvent::Formatted(serde_json::from_value::<WritingFormatResult>(result)?.file)
            }
        })
    }
}

/// A document the engine opened in a pane, with the first lines of its text.
#[derive(Debug, Clone)]
pub struct Opened {
    pub document: WritingDocument,
    pub pane: Pane,
    pub viewport: FileReadResult,
}

/// What went wrong with a request, for the replica to recover.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    List {
        workspace_id: String,
        section: WritingSection,
    },
    Create {
        workspace_id: String,
        section: WritingSection,
    },
    Structure {
        file_id: String,
    },
}

/// What the engine told the writing panes, in the order it happened.
#[derive(Debug, Clone)]
pub enum WritingEvent {
    Listed {
        workspace_id: String,
        section: WritingSection,
        cursor: Option<String>,
        result: WritingSectionListResult,
    },
    Opened(Box<Opened>),
    Structure(Box<WritingStructureResult>),
    Formatted(FileDocument),
    Failed(Failure),
}

/// Carries `command` out and says what the engine's answer tells the replica.
pub(crate) fn run(rpc: &Rpc, command: &Command) -> Result<Option<Event>> {
    let result = rpc.call(&command.request())?;
    Ok(Some(Event::Writing(command.outcome(result)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_asks_for_the_clients_page_of_a_section() {
        let command = Command::List {
            workspace_id: "w".into(),
            section: WritingSection::Texts,
            cursor: Some("c".into()),
        };
        let Request::WritingSectionList(params) = command.request() else {
            panic!("not a list");
        };
        assert_eq!(params.limit, PAGE_SIZE_WRITING_SECTION);
        assert_eq!(params.cursor.as_deref(), Some("c"));
    }

    #[test]
    fn a_format_is_asked_to_the_revision_it_was_chosen_on() {
        let command = Command::Format {
            file_id: "f".into(),
            base: "7".into(),
            range: TextRange {
                start: crate::editor::page::position(0, 1),
                end: crate::editor::page::position(0, 4),
            },
            kind: WritingFormatKind::Bold,
        };
        let Request::WritingFormat(params) = command.request() else {
            panic!("not a format");
        };
        assert_eq!(params.base_revision, "7");
        assert_eq!(params.kind, WritingFormatKind::Bold);
        assert!(params.value.is_none() && params.image.is_none());
    }

    #[test]
    fn a_failure_is_told_where_it_happened_when_nothing_waits_on_it() {
        let list = Command::List {
            workspace_id: "w".into(),
            section: WritingSection::Texts,
            cursor: None,
        };
        let structure = Command::Structure {
            file_id: "f".into(),
            revision: "1".into(),
        };
        assert!(list.failure_is_shown_in_place() && structure.failure_is_shown_in_place());
        assert!(list.failure().is_some() && structure.failure().is_some());
        let open = Command::Open {
            workspace_id: "w".into(),
            path: "texts/a.md".into(),
            kind: WritingKind::Text,
        };
        assert!(!open.failure_is_shown_in_place() && open.failure().is_none());
    }
}

//! What the editor asks of the engine, the typed requests that carry it out, and what the
//! engine's answers tell the replica.

use crate::{Error, Result, commands::key, event::Event, rpc::Rpc};
use serde_json::Value;
use soyeht_protocol::{
    EditorTabsListParams, EditorTabsListResult, FileAuthor, FileFindDirection, FileFindParams,
    FileFindQuery, FileFindResult, FileOpenParams, FileOpenResult, FileReadParams, FileReadResult,
    FileUndoParams, FileUndoResult, FileWriteParams, FileWriteResult, ProjectTreeListParams,
    ProjectTreeListResult, ProjectTreeWatchParams, Request, TextEdit, TextPosition,
    client_policy::{EDITOR_SAVE_ATTEMPTS, EDITOR_SAVE_RETRY_MS},
};
use std::time::Duration;

/// The text the engine is asked for, one page at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRequest {
    pub file_id: String,
    pub from_line: u32,
    pub lines: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditorCommand {
    /// Open a file in a tab: a preview tab, or a permanent one.
    Open {
        workspace_id: String,
        path: String,
        preview: bool,
    },
    /// Read the lines of a page.
    Read(PageRequest),
    /// Make edits to the revision `base`: the engine refuses them when the file moved on.
    Write {
        file_id: String,
        base: String,
        edits: Vec<TextEdit>,
    },
    /// Undo (or redo) the newest edit the engine recorded, then read the page again.
    Replay {
        file_id: String,
        base: String,
        redo: bool,
    },
    /// Read the page until the engine has saved the file.
    Save(PageRequest),
    Tabs {
        workspace_id: String,
    },
    List {
        workspace_id: String,
        path: String,
        show_ignored: bool,
    },
    Watch {
        workspace_id: String,
        paths: Vec<String>,
    },
    Find {
        /// Numbers the search, so that an answer to an older one is ignored.
        seq: u64,
        file_id: String,
        revision: String,
        query: FileFindQuery,
        from: TextPosition,
        forward: bool,
        page: PageRequest,
    },
}

impl EditorCommand {
    /// Fetches that nothing waits on: they run apart from the writes that carry the typing.
    pub fn is_background(&self) -> bool {
        matches!(
            self,
            Self::Read(_)
                | Self::Save(_)
                | Self::Tabs { .. }
                | Self::List { .. }
                | Self::Watch { .. }
                | Self::Find { .. }
        )
    }
}

/// What went wrong with a request, for the replica to recover.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// A write or a replay: the edits the replica made are not the engine's, so it reads the
    /// page again.
    Edit(String),
    /// A read or a save, of the page that starts at `from_line`: nothing waits for it any more.
    /// `reason` is what the pane shows when it holds no page.
    Read {
        file_id: String,
        from_line: u32,
        reason: String,
    },
    /// The tabs of a workspace could not be listed: a pane that waits for its file has none.
    Tabs {
        workspace_id: String,
        reason: String,
    },
    /// A folder that could not be listed.
    List { workspace_id: String, path: String },
}

/// What the engine told the editor, in the order it happened.
#[derive(Debug, Clone)]
pub enum EditorEvent {
    Opened(Box<FileOpenResult>),
    Page(Box<FileReadResult>),
    Written {
        file_id: String,
        result: Box<FileWriteResult>,
    },
    Replayed {
        file_id: String,
        result: Box<FileUndoResult>,
    },
    Tabs(EditorTabsListResult),
    Listed(Box<ProjectTreeListResult>),
    Found {
        seq: u64,
        file_id: String,
        result: FileFindResult,
    },
    Failed(Failure),
}

fn read(page: &PageRequest) -> FileReadParams {
    FileReadParams {
        file_id: page.file_id.clone(),
        revision: None,
        source: None,
        version_id: None,
        from_line: page.from_line,
        line_count: page.lines,
    }
}

/// The single request an editor command needs, written by `author`; `Save` takes several, and
/// `None` says so.
fn request(command: &EditorCommand, author: &FileAuthor) -> Option<Request> {
    Some(match command {
        EditorCommand::Open {
            workspace_id,
            path,
            preview,
        } => Request::FileOpen(FileOpenParams {
            workspace_id: workspace_id.clone(),
            path: path.clone(),
            line: None,
            column: None,
            preview: preview.then_some(true),
            idempotency_key: key(),
        }),
        EditorCommand::Read(page) => Request::FileRead(read(page)),
        EditorCommand::Write {
            file_id,
            base,
            edits,
        } => Request::FileWrite(FileWriteParams {
            file_id: file_id.clone(),
            base_revision: base.clone(),
            edits: edits.clone(),
            author: author.clone(),
            idempotency_key: key(),
        }),
        EditorCommand::Replay {
            file_id,
            base,
            redo,
        } => {
            let params = FileUndoParams {
                file_id: file_id.clone(),
                base_revision: base.clone(),
                idempotency_key: key(),
            };
            if *redo {
                Request::FileRedo(params)
            } else {
                Request::FileUndo(params)
            }
        }
        EditorCommand::Save(_) => return None,
        EditorCommand::Tabs { workspace_id } => Request::EditorTabsList(EditorTabsListParams {
            workspace_id: workspace_id.clone(),
        }),
        EditorCommand::List {
            workspace_id,
            path,
            show_ignored,
        } => Request::ProjectTreeList(ProjectTreeListParams {
            workspace_id: workspace_id.clone(),
            path: path.clone(),
            show_ignored: show_ignored.then_some(true),
        }),
        EditorCommand::Watch {
            workspace_id,
            paths,
        } => Request::ProjectTreeWatch(ProjectTreeWatchParams {
            workspace_id: workspace_id.clone(),
            paths: paths.clone(),
        }),
        EditorCommand::Find {
            file_id,
            revision,
            query,
            from,
            forward,
            page,
            ..
        } => Request::FileFind(FileFindParams {
            file_id: file_id.clone(),
            revision: revision.clone(),
            query: query.clone(),
            from: from.clone(),
            direction: if *forward {
                FileFindDirection::Next
            } else {
                FileFindDirection::Previous
            },
            visible_from_line: Some(page.from_line),
            visible_line_count: Some(page.lines),
        }),
    })
}

/// Carries `command` out and says what the engine's answer tells the editor.
pub(crate) fn run(rpc: &Rpc, command: &EditorCommand) -> Result<Option<Event>> {
    if let EditorCommand::Save(page) = command {
        return save(rpc, page).map(Some);
    }
    match request(command, &rpc.author()?) {
        Some(request) => outcome(command, rpc.call(&request)?),
        None => Ok(None),
    }
}

/// What the engine's answer to `command` tells the editor.
fn outcome(command: &EditorCommand, result: Value) -> Result<Option<Event>> {
    let event = match command {
        EditorCommand::Open { .. } => {
            EditorEvent::Opened(Box::new(serde_json::from_value(result)?))
        }
        EditorCommand::Read(_) | EditorCommand::Save(_) => {
            EditorEvent::Page(Box::new(serde_json::from_value(result)?))
        }
        EditorCommand::Write { file_id, .. } => EditorEvent::Written {
            file_id: file_id.clone(),
            result: Box::new(serde_json::from_value(result)?),
        },
        EditorCommand::Replay { file_id, .. } => EditorEvent::Replayed {
            file_id: file_id.clone(),
            result: Box::new(serde_json::from_value(result)?),
        },
        EditorCommand::Tabs { .. } => EditorEvent::Tabs(serde_json::from_value(result)?),
        EditorCommand::List { .. } => {
            EditorEvent::Listed(Box::new(serde_json::from_value(result)?))
        }
        EditorCommand::Watch { .. } => return Ok(None),
        EditorCommand::Find { seq, file_id, .. } => EditorEvent::Found {
            seq: *seq,
            file_id: file_id.clone(),
            result: serde_json::from_value(result)?,
        },
    };
    Ok(Some(Event::Editor(event)))
}

/// What the replica learns when `command` fails for `reason`, so that it does not wait for an
/// answer.
pub(crate) fn failure(command: &EditorCommand, reason: &str) -> Option<Event> {
    let failure = match command {
        EditorCommand::Write { file_id, .. } | EditorCommand::Replay { file_id, .. } => {
            Failure::Edit(file_id.clone())
        }
        EditorCommand::Read(PageRequest {
            file_id, from_line, ..
        })
        | EditorCommand::Save(PageRequest {
            file_id, from_line, ..
        }) => Failure::Read {
            file_id: file_id.clone(),
            from_line: *from_line,
            reason: reason.to_owned(),
        },
        EditorCommand::Tabs { workspace_id } => Failure::Tabs {
            workspace_id: workspace_id.clone(),
            reason: reason.to_owned(),
        },
        EditorCommand::List {
            workspace_id, path, ..
        } => Failure::List {
            workspace_id: workspace_id.clone(),
            path: path.clone(),
        },
        _ => return None,
    };
    Some(Event::Editor(EditorEvent::Failed(failure)))
}

/// Reads `page` until the file is no longer dirty: the engine saves a draft shortly after the
/// last edit, so the page read at that moment is the saved one.
fn save(rpc: &Rpc, page: &PageRequest) -> Result<Event> {
    for attempt in 0..EDITOR_SAVE_ATTEMPTS {
        let result = rpc.call(&Request::FileRead(read(page)))?;
        let read: FileReadResult = serde_json::from_value(result)?;
        if !read.file.dirty || attempt + 1 == EDITOR_SAVE_ATTEMPTS {
            let saved = !read.file.dirty;
            let event = Event::Editor(EditorEvent::Page(Box::new(read)));
            return if saved {
                Ok(event)
            } else {
                Err(Error::Invalid("the file did not finish saving in time"))
            };
        }
        std::thread::sleep(Duration::from_millis(u64::from(EDITOR_SAVE_RETRY_MS)));
    }
    Err(Error::Invalid("the file did not finish saving in time"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn author() -> FileAuthor {
        FileAuthor {
            actor: soyeht_protocol::Actor {
                role: soyeht_protocol::Role::HumanLocal,
                client: "desktop".into(),
                device_id: None,
                pane_id: None,
            },
            name: "you".into(),
        }
    }

    #[test]
    fn a_page_is_read_without_pinning_a_revision() {
        let command = EditorCommand::Read(PageRequest {
            file_id: "f".into(),
            from_line: 40,
            lines: 100,
        });
        let Some(Request::FileRead(params)) = request(&command, &author()) else {
            panic!("no read");
        };
        assert_eq!((params.from_line, params.line_count), (40, 100));
        assert!(params.revision.is_none() && params.source.is_none());
    }

    #[test]
    fn a_save_is_a_command_of_several_requests() {
        let page = PageRequest {
            file_id: "f".into(),
            from_line: 0,
            lines: 10,
        };
        assert!(request(&EditorCommand::Save(page), &author()).is_none());
    }

    #[test]
    fn a_failed_write_is_told_to_the_replica() {
        let command = EditorCommand::Write {
            file_id: "f".into(),
            base: "3".into(),
            edits: Vec::new(),
        };
        let Some(Event::Editor(EditorEvent::Failed(Failure::Edit(file)))) =
            failure(&command, "refused")
        else {
            panic!("no failure");
        };
        assert_eq!(file, "f");
        assert!(
            failure(
                &EditorCommand::Watch {
                    workspace_id: "w".into(),
                    paths: Vec::new()
                },
                "refused"
            )
            .is_none()
        );
    }
}

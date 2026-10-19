//! File ownership, viewport projection, and filesystem isolation.

pub(crate) mod agent_review;
pub(crate) mod api;
mod creation;
#[cfg(test)]
mod effect_backoff_tests;
mod find;
#[cfg(test)]
mod keystroke_tests;
mod persistence;
mod project_entries;
mod project_files;
mod project_paths;
mod project_reader;
#[cfg(test)]
mod project_tests;
mod project_tree;
mod project_watch;
#[cfg(test)]
mod project_watch_tests;
#[cfg(test)]
mod save_temporary_tests;
mod schedule;
mod search;
mod service;
mod tabs;
#[cfg(test)]
pub(crate) mod test_fixture;
#[cfg(test)]
mod test_support;
mod text_refusal;
mod trash;
mod undo;

pub(crate) use project_files::{ProjectFile, checked_wire_path, enumerate_project_files, language};
pub use project_watch::Watches as ProjectWatches;
pub(crate) use schedule::{Effect, Failure};
pub use service::{FileSystemPort, Service, SystemFiles};
pub(crate) use tabs::refresh_tabs;

use crate::{core::Core, errors, errors::Result};
use serde_json::Value;
use soyeht_protocol::method::*;

pub fn handles(method: &str) -> bool {
    matches!(
        method,
        FILE_OPEN
            | FILE_READ
            | FILE_WRITE
            | FILE_WATCH
            | FILE_HISTORY
            | FILE_REVERT
            | FILE_UNDO
            | FILE_REDO
            | FILE_SEARCH
            | FILE_FIND
            | FILE_REPLACE
            | PROJECT_TREE_LIST
            | PROJECT_TREE_WATCH
            | PROJECT_ENTRY_CREATE
            | PROJECT_ENTRY_RENAME
            | PROJECT_ENTRY_DELETE
            | EDITOR_TABS_LIST
            | EDITOR_TABS_MOVE
            | EDITOR_REVIEW_ACCEPT
            | EDITOR_REVIEW_REJECT
            | EDITOR_REVIEW_COMMENT
    )
}

impl Core {
    pub fn editor_request(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        match method {
            FILE_OPEN => self.file_open(connection, params),
            FILE_READ => self.file_read(params),
            FILE_WRITE => self.file_write(connection, params),
            FILE_WATCH => self.file_watch(connection, params),
            FILE_HISTORY => self.file_history(params),
            FILE_REVERT => self.file_revert(connection, params),
            FILE_UNDO | FILE_REDO => self.file_undo(connection, method, params),
            FILE_FIND => self.file_find(params),
            FILE_REPLACE => self.file_replace(connection, params),
            PROJECT_TREE_LIST => self.project_tree_list(params),
            PROJECT_TREE_WATCH => self.project_tree_watch(connection, params),
            PROJECT_ENTRY_CREATE => self.project_entry_create(connection, params),
            PROJECT_ENTRY_RENAME => self.project_entry_rename(connection, params),
            PROJECT_ENTRY_DELETE => self.project_entry_delete(connection, params),
            EDITOR_TABS_LIST => self.editor_tabs_list(params),
            EDITOR_TABS_MOVE => self.editor_tabs_move(connection, params),
            EDITOR_REVIEW_ACCEPT | EDITOR_REVIEW_REJECT => {
                self.editor_review(connection, method, params)
            }
            EDITOR_REVIEW_COMMENT => self.editor_review_comment(connection, params),
            _ => Err(errors::internal("Unknown editor method")),
        }
    }
}

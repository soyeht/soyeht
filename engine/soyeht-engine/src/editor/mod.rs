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
mod trash;
mod undo;

pub(crate) use project_files::{ProjectFile, enumerate_project_files, language};
pub use project_watch::Watches as ProjectWatches;
pub(crate) use schedule::{Effect, Failure};
pub use service::{FileSystemPort, Service, SystemFiles};
pub(crate) use tabs::refresh_tabs;

use crate::{core::Core, errors, errors::Result};
use serde_json::Value;

pub fn handles(method: &str) -> bool {
    matches!(
        method,
        "file.open"
            | "file.read"
            | "file.write"
            | "file.watch"
            | "file.history"
            | "file.revert"
            | "file.undo"
            | "file.redo"
            | "file.search"
            | "file.find"
            | "file.replace"
            | "project.tree.list"
            | "project.tree.watch"
            | "project.entry.create"
            | "project.entry.rename"
            | "project.entry.delete"
            | "editor.tabs.list"
            | "editor.tabs.move"
            | "editor.review.accept"
            | "editor.review.reject"
            | "editor.review.comment"
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
            "file.open" => self.file_open(connection, params),
            "file.read" => self.file_read(params),
            "file.write" => self.file_write(connection, params),
            "file.watch" => self.file_watch(connection, params),
            "file.history" => self.file_history(params),
            "file.revert" => self.file_revert(connection, params),
            "file.undo" | "file.redo" => self.file_undo(connection, method, params),
            "file.find" => self.file_find(params),
            "file.replace" => self.file_replace(connection, params),
            "project.tree.list" => self.project_tree_list(params),
            "project.tree.watch" => self.project_tree_watch(connection, params),
            "project.entry.create" => self.project_entry_create(connection, params),
            "project.entry.rename" => self.project_entry_rename(connection, params),
            "project.entry.delete" => self.project_entry_delete(connection, params),
            "editor.tabs.list" => self.editor_tabs_list(params),
            "editor.tabs.move" => self.editor_tabs_move(connection, params),
            "editor.review.accept" | "editor.review.reject" => {
                self.editor_review(connection, method, params)
            }
            "editor.review.comment" => self.editor_review_comment(connection, params),
            _ => Err(errors::internal("Unknown editor method")),
        }
    }
}

//! File ownership, viewport projection, and filesystem isolation.

pub(crate) mod agent_review;
pub(crate) mod api;
mod persistence;
mod search;
mod service;

pub(crate) use service::language;
pub use service::{FileSystemPort, Service, SystemFiles};

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
            | "file.search"
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
            "file.search" => self.file_search(connection, params),
            "editor.review.accept" | "editor.review.reject" => {
                self.editor_review(connection, method, params)
            }
            "editor.review.comment" => self.editor_review_comment(connection, params),
            _ => Err(errors::internal("Unknown editor method")),
        }
    }
}

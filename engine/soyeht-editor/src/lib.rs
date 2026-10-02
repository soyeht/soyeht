//! Pure editor buffer, undo history, viewport, and incremental syntax model.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod buffer;
mod error;
mod find;
mod history;
mod search;
mod syntax;
mod viewport;

pub use buffer::{AppliedEdit, TextBuffer};
pub use error::{Error, Result};
pub use find::{
    FindDirection, FindOutcome, FindRequest, FoundMatch, find, replace_all, replace_one,
};
pub use history::{AppliedStep, EditHistory, EditStep, GROUP_MS, HistoryDirection, Replay};
pub use search::{
    ContentMatch, ContentMatcher, ContentSearch, MatchOptions, PathSearchMatch, rank_paths,
    search_content,
};
pub use syntax::SyntaxDocument;
pub use viewport::{project, project_bounded};

//! Pure editor buffer, viewport, and incremental syntax model.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod buffer;
mod error;
mod search;
mod syntax;
mod viewport;

pub use buffer::{AppliedEdit, TextBuffer};
pub use error::{Error, Result};
pub use search::{ContentMatch, ContentSearch, PathSearchMatch, rank_paths, search_content};
pub use syntax::SyntaxDocument;
pub use viewport::{project, project_bounded};

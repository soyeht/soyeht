//! Pure editor buffer, viewport, and incremental syntax model.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod buffer;
mod error;
mod syntax;
mod viewport;

pub use buffer::{AppliedEdit, TextBuffer};
pub use error::{Error, Result};
pub use syntax::SyntaxDocument;
pub use viewport::{project, project_bounded};

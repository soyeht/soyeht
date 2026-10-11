//! Pure, bounded document structure and mutation rules for writing surfaces.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod beats;
mod error;
mod markdown;
mod screenplay;
mod sections;
mod slides;
mod variants;

pub use beats::BeatBoard;
pub use error::{Error, Result};
pub use markdown::{HybridBlock, HybridBlockKind, format_markdown, hybrid_blocks};
pub use screenplay::{Screenplay, advance_after, parse_screenplay};
pub use sections::{SectionEntryInput, folder_for, kind_for, new_document_path, section_entries};
pub use slides::segment_slides;
pub use variants::VariantSet;

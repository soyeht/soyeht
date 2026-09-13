//! Bounded source-control snapshots for editor buffers.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod diff;
mod error;
mod model;
mod port;
mod service;

pub use error::{Error, Result};
pub use model::{Blame, Diff, Hunk, Line, LineKind};
pub use port::{CommandPort, SystemGit};
pub use service::Service;

//! Public wire models and bounded framing for Soyeht clients.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod generated;
pub mod limits;
pub use generated::*;
pub mod attachment;
mod error;
pub use error::{Error, Result};
pub mod frame;
pub mod ndjson;
pub mod profile;
pub mod validate;

/// The wire schema is embedded for tooling and compatibility diagnostics.
pub const SCHEMA: &str = include_str!("../../../contracts/protocol.json");

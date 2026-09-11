//! Public wire models and bounded framing for Soyeht clients.

mod generated;
pub mod limits;
pub use generated::*;
pub mod frame;

/// The wire schema is embedded for tooling and compatibility diagnostics.
pub const SCHEMA: &str = include_str!("../../../contracts/protocol.json");

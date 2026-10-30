//! Public wire models and bounded framing for Soyeht clients.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod client_policy;
#[cfg(feature = "contract-checks")]
pub mod contract;
mod generated;
pub mod limits;
pub use generated::*;
pub mod attachment;
pub mod data_directory;
mod error;
pub use error::{Error, Result, SUPERVISOR_DISCONNECTED};
pub mod frame;
pub mod handshake;
pub mod legacy;
pub mod ndjson;
pub mod profile;
pub mod readiness;
pub mod setting;
pub mod snapshot;
pub mod update;
pub mod update_policy;
pub mod validate;
pub mod web;

/// The wire schema is embedded for tooling and compatibility diagnostics.
pub const SCHEMA: &str = include_str!("../../../contracts/protocol.json");

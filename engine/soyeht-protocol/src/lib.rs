//! Public wire models and bounded framing for Soyeht clients.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod client_policy;
#[cfg(feature = "contract-checks")]
pub mod contract;
mod generated;
pub mod limits;
pub use generated::*;
pub mod approval_code;
pub mod attachment;
pub mod data_directory;
pub mod device_auth;
mod error;
pub use error::{Error, Result, SUPERVISOR_DISCONNECTED};
pub mod frame;
pub mod handshake;
pub mod legacy;
pub mod ndjson;
pub mod pairing_link;
pub mod profile;
pub mod readiness;
pub mod service_report;
pub mod setting;
pub mod shared_text;
pub mod snapshot;
pub mod update;
pub mod update_policy;
pub mod validate;
pub mod web;

/// The wire schema is embedded for tooling and compatibility diagnostics.
pub const SCHEMA: &str = include_str!("../../../contracts/protocol.json");

impl Role {
    /// Whether the actor is a person, at this machine or on a paired device, and not an agent.
    pub fn is_person(&self) -> bool {
        matches!(self, Self::HumanLocal | Self::HumanDevice)
    }
}

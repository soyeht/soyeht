//! Language Server Protocol framing, correlation, and typed normalization.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod client;
mod config;
mod error;
mod frame;
mod model;
mod restricted;
mod transport;

pub use client::{
    Call, Client, Incoming, PositionValue, RequestKind, did_change, did_open, edits, initialized,
    locations, symbols,
};
pub use config::{LanguageConfig, config, refusal};
pub use error::{Error, Result};
pub use frame::{Decoder, encode};
pub use model::{Diagnostic, Location, Position, Range, Symbol, TextEdit};
pub use restricted::{LaunchError, Restricted, restricted_launch};
pub use transport::{MemoryTransport, TransportPort, drive_once};

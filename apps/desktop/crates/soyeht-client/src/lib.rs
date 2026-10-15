//! The Soyeht desktop client's connection to the engine, with no window system in sight.
//!
//! Layers, from the wire up: `transport` (a byte stream) -> `rpc` and `session` (requests,
//! handshake, routing of everything the engine sends) -> `engine` (a supervised connection and
//! a command queue) -> `state` (the replica an interface renders). Wire types, framing and
//! method names all come from `soyeht-protocol`; nothing about the protocol is defined here.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod account;
mod account_events;
pub mod agent;
mod agents;
pub mod attachments;
pub mod cards;
pub mod catalogue;
pub mod commands;
mod engine;
mod error;
mod event;
pub mod harness_picker;
pub mod keys;
pub mod layout;
pub mod loops;
pub mod markdown;
mod paging;
pub mod permission_preview;
mod preference_requests;
pub mod preferences;
pub mod profile;
mod reconnect;
mod rpc;
pub mod screen;
mod session;
mod snapshot;
pub mod state;
pub mod streams;
pub mod switch;
pub mod tasks;
pub mod terminal_state;
#[cfg(test)]
mod testing;
pub mod timestamp;
pub mod transcript_rows;
pub mod transport;
pub mod usage;

pub use commands::Command;
pub use engine::{CLIENT_NAME, Engine};
pub use error::{Error, Result};
pub use event::Event;
pub use session::Session;
pub use snapshot::Snapshot;
pub use state::{Connection, Effect, State};

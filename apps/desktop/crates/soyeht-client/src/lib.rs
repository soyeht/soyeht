//! The Soyeht desktop client's connection to the engine, with no window system in sight.
//!
//! Layers, from the wire up: `transport` (a byte stream) -> `rpc` and `session` (requests,
//! handshake, routing of everything the engine sends) -> `engine` (a supervised connection and
//! a command queue) -> `state` (the replica an interface renders). Wire types, framing and
//! method names all come from `soyeht-protocol`; nothing about the protocol is defined here.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod commands;
mod engine;
mod error;
mod event;
pub mod keys;
pub mod profile;
mod rpc;
pub mod screen;
mod session;
mod snapshot;
pub mod state;
pub mod streams;
#[cfg(test)]
mod testing;
pub mod transport;

pub use commands::Command;
pub use engine::Engine;
pub use error::{Error, Result};
pub use event::Event;
pub use session::Session;
pub use snapshot::Snapshot;
pub use state::{Connection, Effect, State};

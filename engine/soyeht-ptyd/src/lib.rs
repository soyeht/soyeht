#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod client;
mod error;
pub use error::{Error, Result};
mod pending;
pub mod pipe_guard;
mod process;
mod registry;
pub mod retention;
pub mod server;
pub mod wire;

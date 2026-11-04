#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod child;
pub mod client;
mod error;
pub use error::{ENVIRONMENT_NOT_ALLOWED, Error, Result};
pub mod io;
mod pending;
pub mod pipe_guard;
mod process;
pub use process::CLAUDE_CODE_EXECUTABLE;
#[cfg(windows)]
pub use process::SYSTEM_ENVIRONMENT;
pub mod pty;
mod registry;
pub mod retention;
pub mod server;
pub mod shell;
pub mod wire;
pub mod wire_kind;

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod engine;
mod error;
pub mod server;
pub mod tools;

pub use error::{Error, Result};

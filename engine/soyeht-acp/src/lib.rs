//! ACP v1 adaptation. Process ownership and durable effects belong to the engine.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
pub mod client;
pub mod commands;
pub mod config;
pub mod continuation;
pub mod rpc;
pub mod session;
pub mod subagents;

pub use client::Client;
pub use rpc::{Error, Message, RpcError};
pub type Result<T> = std::result::Result<T, Error>;

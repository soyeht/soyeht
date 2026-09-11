//! ACP v1 adaptation. Process ownership and durable effects belong to the engine.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
pub mod client;
pub mod config;
pub mod rpc;
pub mod session;

pub use client::Client;
pub use rpc::{Error, Message, RpcError};

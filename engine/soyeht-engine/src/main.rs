#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use soyeht_engine::{errors::Result, hooks::NoHooks, launch};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    launch::run(|_| Arc::new(NoHooks)).await
}

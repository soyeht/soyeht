//! The engine with the hooks of the integration tests and the end-to-end suite installed
//! (ADR 0091). Built only with the `test-hooks` feature; never part of a product.

use soyeht_engine::{errors::Result, launch, test_hooks::FileHooks};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    launch::run(|directory| Arc::new(FileHooks::from_environment(directory))).await
}

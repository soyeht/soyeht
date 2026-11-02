//! The engine with the hooks of the integration tests and the end-to-end suite installed
//! (ADR 0091). Built only with the `test-hooks` feature; never part of a product.

#![allow(clippy::panic)] // The crash hook test makes the engine panic on purpose.

use soyeht_engine::{errors::Result, launch, test_hooks::FileHooks};
use std::sync::Arc;

/// A marker in the data directory that makes the engine panic on its main thread as it starts,
/// after the crash report hook is installed (ADR 0127).
const CRASH_AT_START: &str = "test-crash-at-start";

#[tokio::main]
async fn main() -> Result<()> {
    launch::run(|directory| {
        if directory.join(CRASH_AT_START).exists() {
            panic!("the test asked the engine to crash at start");
        }
        Arc::new(FileHooks::from_environment(directory))
    })
    .await
}

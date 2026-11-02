#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use soyeht_engine::{errors::startup_failure, hooks::NoHooks, launch};
use std::{process::ExitCode, sync::Arc};

#[tokio::main]
async fn main() -> ExitCode {
    match launch::run(|_| Arc::new(NoHooks)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Copy diagnostics keeps only structured lines, so the reason it will not start is one:
            // its kind, never the text, which can carry a path (ADR 0088).
            soyeht_platform::log::failure("engine.startup_failed", &error);
            // The text a person reads in the terminal or the log: not the `Debug` form.
            eprintln!("soyeht-engine: {}", startup_failure(&error));
            ExitCode::FAILURE
        }
    }
}

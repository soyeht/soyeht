#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use soyeht_engine::{errors::startup_failure, hooks::NoHooks, launch};
use std::{process::ExitCode, sync::Arc};

#[tokio::main]
async fn main() -> ExitCode {
    match launch::run(|_| Arc::new(NoHooks)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The text a person reads in the terminal or the log: not the `Debug` form.
            eprintln!("soyeht-engine: {}", startup_failure(&error));
            ExitCode::FAILURE
        }
    }
}

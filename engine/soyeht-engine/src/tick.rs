//! The engine tick: apply background results, drain terminal output, pump
//! streams. Each tick holds the Core lock briefly (ADR 0079). The usage
//! schedule starts here too.

use crate::{
    core_lock::{self, Shared},
    errors,
};
use std::time::Duration;

const TICK_INTERVAL: Duration = Duration::from_millis(10);
/// Ticks between store prunes: once a minute.
const PRUNE_EVERY_TICKS: u32 = 6_000;

/// Runs the tick, and starts the usage schedule beside it. The schedule may
/// end (the test host drives refreshes itself) without ending the engine.
pub(crate) async fn run(core: Shared) -> crate::errors::Result<()> {
    tokio::spawn(crate::usage::schedule(std::sync::Arc::clone(&core)));
    let mut ticks = tokio::time::interval(TICK_INTERVAL);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut prune = 0_u32;
    let mut previous_error = None;
    loop {
        ticks.tick().await;
        let result = core_lock::with(&core, "tick", move |core| {
            let output = core.poll_output();
            let streams = core.pump_streams();
            if prune == 0 {
                core.store.prune().map_err(crate::errors::storage)?;
            }
            output.and(streams)
        })
        .await;
        prune = (prune + 1) % PRUNE_EVERY_TICKS;
        let error = result.err().map(|error| errors::kind(&error));
        if error != previous_error {
            if let Some(kind) = &error {
                eprintln!(
                    "{}",
                    serde_json::json!({"event":"engine.storage_or_supervisor_error","kind":kind})
                );
            }
            previous_error = error;
        }
    }
}

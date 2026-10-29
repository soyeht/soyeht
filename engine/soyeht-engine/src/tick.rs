//! The engine tick: apply background results, drain terminal output, pump
//! streams. Each tick holds the Core lock briefly (ADR 0079). The usage
//! schedule starts here too.

use crate::{
    core_lock::{self, Shared},
    streak::Streak,
};
use soyeht_platform::log::{self, Failure as _};
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
    let mut streak = Streak::once();
    let woken = core_lock::with(&core, "tick_wake", |core| Ok(core.background.woken())).await?;
    loop {
        let result = tokio::select! {
            _ = ticks.tick() => {
                let due_prune = prune == 0;
                prune = (prune + 1) % PRUNE_EVERY_TICKS;
                core_lock::with(&core, "tick", move |core| {
                    let output = core.poll_output();
                    let streams = core.pump_streams();
                    if due_prune {
                        core.store.prune().map_err(crate::errors::storage)?;
                    }
                    output.and(streams)
                })
                .await
            }
            // A job asked to be applied at once (a read that found output): apply it, poll what
            // the read was for and send what it brought now, and leave the rest of the tick to
            // its turn.
            () = woken.notified() => {
                core_lock::with(&core, "background", |core| {
                    core.apply_background();
                    let polled = core.poll_woken();
                    let streams = core.pump_streams();
                    polled.and(streams)
                })
                .await
            }
        };
        crate::slow_holds::emit_due();
        match result {
            Ok(()) => {
                streak.succeed();
            }
            Err(error) => {
                if streak.fail(&error.failure_kind()) {
                    log::failure("engine.storage_or_supervisor_error", &error);
                }
            }
        }
    }
}

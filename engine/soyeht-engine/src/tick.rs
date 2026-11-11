//! The engine tick: apply background results, drain terminal output, pump
//! streams. Each tick holds the Core lock briefly (ADR 0079). The usage
//! schedule starts here too.
//!
//! The tick does not run on a period (ADR 0147, step 1b). Each run ends by asking
//! [`Core::tick_wait`] how long the subsystems can do without it, and the loop sleeps that long,
//! or until something raises it. What raises it, and why nothing is left to a timer alone:
//!
//! - a **kick** (`Background::kicker`) says something may have changed: a background job ended,
//!   a request or any other hold of the Core lock that is not the tick's own ended, a file event
//!   arrived. The tick runs once the floor (one tick interval since its last run) has passed;
//! - a **wake** (`Background::woken`) says a result is worth applying at once, as before: a read
//!   that found output, a notice of the supervisor. The tick applies it and pumps now, and runs
//!   in full after the floor;
//! - a **deadline** is what a subsystem asked for in `tick_due`.
//!
//! A run that wrote to the store runs again after the floor, since what it wrote may have given
//! another subsystem work.

use crate::{
    core::Core,
    core_lock::{self, Shared},
    streak::Streak,
    tick_due::FLOOR,
};
use soyeht_platform::log::{self, Failure as _};
use std::time::Duration;
use tokio::time::Instant;

/// The purposes of the holds that are the tick's own: they do not raise it.
pub(crate) const OWN_HOLDS: [&str; 3] = ["tick", "background", "tick_wake"];
/// Between store prunes.
const PRUNE_EVERY: Duration = Duration::from_secs(60);

/// What one full run of the tick found.
struct Run {
    result: crate::errors::Result<()>,
    /// How long the subsystems can do without the tick from here.
    wait: Duration,
}

/// One full run of the tick, under the Core lock.
fn run_once(core: &mut Core, due_prune: bool) -> Run {
    let changes = core.store.change_counter();
    let output = core.poll_output();
    let streams = core.pump_streams();
    let pruned = if due_prune {
        core.store.prune().map_err(crate::errors::storage)
    } else {
        Ok(())
    };
    Run {
        result: pruned.and(output.and(streams)),
        wait: wait_after(core, changes),
    }
}

/// How long the tick may sleep after a run that began when the store had written `changes` rows.
/// A run that wrote to the store may have given another subsystem work.
fn wait_after(core: &Core, changes: u64) -> Duration {
    if core.store.change_counter() == changes {
        core.tick_wait()
    } else {
        Duration::ZERO
    }
}

/// Runs the tick, and starts the usage schedule beside it. The schedule may
/// end (the test host drives refreshes itself) without ending the engine.
pub(crate) async fn run(core: Shared) -> crate::errors::Result<()> {
    tokio::spawn(crate::usage::schedule(std::sync::Arc::clone(&core)));
    let mut streak = Streak::once();
    let (woken, kick) = core_lock::with(&core, "tick_wake", |core| {
        Ok((core.background.woken(), core.background.kicker()))
    })
    .await?;
    // The first run is at once.
    let mut last_start = Instant::now();
    let mut next = last_start;
    let mut next_prune = last_start;
    loop {
        let full = tokio::select! {
            () = tokio::time::sleep_until(next) => true,
            // A job asked to be applied at once (a read that found output): apply it, poll what
            // the read was for and send what it brought now, and leave the rest of the tick to
            // its turn.
            () = woken.notified() => false,
            // Something may have changed: run in full, no sooner than the floor.
            () = kick.notified() => {
                next = next.min(last_start + FLOOR);
                continue;
            }
        };
        let result = if full {
            let started = Instant::now();
            last_start = started;
            let due_prune = started >= next_prune;
            if due_prune {
                next_prune = started + PRUNE_EVERY;
            }
            let ran =
                core_lock::with(&core, "tick", move |core| Ok(run_once(core, due_prune))).await;
            let (result, wait) = match ran {
                Ok(run) => (run.result, run.wait),
                Err(error) => (Err(error), FLOOR),
            };
            let now = Instant::now();
            let wait = wait
                .min(crate::slow_holds::next_emit_in().unwrap_or(Duration::MAX))
                .min(next_prune.saturating_duration_since(now));
            next = (started + FLOOR).max(now + wait);
            result
        } else {
            let applied = core_lock::with(&core, "background", |core| {
                core.apply_background();
                let polled = core.poll_woken();
                let streams = core.pump_streams();
                polled.and(streams)
            })
            .await;
            // What the result brought goes on from a full run.
            next = next.min(last_start + FLOOR);
            applied
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ports::StorePort, tick_due::CEILING};
    use serde_json::json;
    use soyeht_protocol::Workspace;

    #[test]
    fn a_run_that_wrote_to_the_store_runs_again_and_a_quiet_one_sleeps() {
        let (mut core, directory) = crate::tick_due::tests::quiet_core();
        let changes = core.store.change_counter();
        assert_eq!(wait_after(&core, changes), CEILING, "nothing was written");

        let workspace = Workspace {
            workspace_id: "second".into(),
            name: "Second".into(),
            path: directory.to_string_lossy().into(),
            layout: json!({}),
        };
        StorePort::mutate(&mut core.store, "test", "second", "seed", |change| {
            change.insert_workspace(&workspace)?;
            Ok((json!({}), Vec::new()))
        })
        .unwrap();
        assert_eq!(wait_after(&core, changes), Duration::ZERO);
        // The polls that rested before the write look again, and rest.
        core.poll_output().unwrap();
        assert_eq!(
            wait_after(&core, core.store.change_counter()),
            CEILING,
            "the next run finds the store as it was left"
        );
        crate::test_paths::remove_after(core, directory);
    }
}

//! The engine that stops gives the supervisor the confirmations that recording the waiting
//! outcomes queued (ADR 0147, step R), which no tick is left to send.

use crate::{
    core_lock::{self, Shared},
    errors,
};
use std::time::{Duration, Instant};

/// The rounds of waiting for the sends to the supervisor that the engine queued as it stopped
/// (the confirmations of the outcomes it recorded): one runs, then the one for what arrived
/// meanwhile.
const ROUNDS: usize = 3;
/// The longest the engine waits for the supervisor in all, whatever the rounds: a supervisor that
/// does not answer must not hold the exit, and the next engine confirms what was not
/// (`reconcile_registry`). A round that runs out of it ends the rounds.
const WAIT: Duration = Duration::from_secs(2);

/// Gives the supervisor the confirmations of the outcomes recorded at shutdown, which no tick is
/// left to send.
pub(crate) async fn send(core: &Shared) {
    let ends = Instant::now() + WAIT;
    for _ in 0..ROUNDS {
        let running = core_lock::with(core, "shutdown_follow_ups", |state| {
            Ok(state.send_follow_ups_before_exit())
        })
        .await;
        let done = match running {
            Ok(Some(done)) => done,
            Ok(None) => return,
            Err(error) => {
                errors::log_if_failed("engine.follow_ups_not_sent", Err(error));
                return;
            }
        };
        let left = ends.saturating_duration_since(Instant::now());
        // A wait that fails to join or runs out of time ends the rounds: the same job would be
        // waited for again, and the next engine confirms what was not.
        let ended = tokio::task::spawn_blocking(move || done.wait(left)).await;
        if !ended.unwrap_or(false) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Core;
    use std::sync::Arc;

    /// A core with the outcome of an input recorded at shutdown, whose supervisor takes `delay`
    /// to confirm it and says which operations it confirmed. Jobs run on threads, as in the engine.
    fn core_confirming_slowly(
        delay: Duration,
    ) -> (
        Shared,
        std::path::PathBuf,
        Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        use soyeht_ptyd::wire::{Request, Response};
        let directory =
            std::env::temp_dir().join(format!("soyeht-last-follow-ups-{}", uuid::Uuid::new_v4()));
        let mut core = Core::open(&directory).unwrap();
        let confirmed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = Arc::clone(&confirmed);
        core.link_supervisor(Box::new(crate::test_panes::Answering(move |request| {
            if let Request::ConfirmInput { operation_key } = request {
                std::thread::sleep(delay);
                record.lock().unwrap().push(operation_key);
            }
            Ok(Response::Ok)
        })));
        core.background = crate::offload::Background::default();
        core.input_receipts
            .push(crate::input_receipts::Waiting::for_test("a", "op-a", 0));
        core.flush_all_input_receipts().unwrap();
        (Arc::new(std::sync::Mutex::new(core)), directory, confirmed)
    }

    #[tokio::test]
    async fn the_engine_waits_for_the_confirmation_of_what_it_recorded_as_it_stopped() {
        let (shared, directory, confirmed) = core_confirming_slowly(Duration::from_millis(400));
        send(&shared).await;
        assert_eq!(*confirmed.lock().unwrap(), ["op-a"]);
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn a_supervisor_that_does_not_answer_holds_the_exit_for_one_bound_not_one_a_round() {
        let (shared, directory, _) = core_confirming_slowly(Duration::from_secs(8));
        let started = Instant::now();
        send(&shared).await;
        assert!(
            started.elapsed() < WAIT + Duration::from_millis(500),
            "the exit waited {:?}",
            started.elapsed()
        );
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

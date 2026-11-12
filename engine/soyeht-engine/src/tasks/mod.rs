//! Workspace task rules and their persistence boundary.

mod api;
pub(crate) mod blocking;
pub(crate) mod dispatching;
pub mod drift;
mod events;
pub mod feedback;
mod lifecycle;
pub(crate) mod looping;
mod mode_cap;
mod movement;
mod phrase;
pub mod ports;
mod review;
mod review_request;
pub mod revision;
pub(crate) mod roles;
mod rules;
pub mod worktree;
mod worktree_clean;

pub(crate) use lifecycle::TaskOwnerLoss;
pub(crate) use movement::TaskHumanMove;
pub use phrase::{PhraseFacts, phrase, review_badge};
pub(crate) use rules::next_version;
#[cfg(test)]
pub(crate) use rules::tests::task_fixture;
pub use rules::{
    ExecutorIdentity, apply_block, apply_complete, apply_move, apply_owner_loss, apply_release,
    apply_request_review, apply_review_return, apply_unblock, apply_update, new_loop_config,
    new_task, validate_dependencies, validate_executor,
};

use crate::{core::Core, errors::Result};

/// How long a task poll rests when it ends without advancing its rows: the tick asks again then,
/// instead of running at its floor for a poll that never rests (ADR 0147, step 1b).
pub(crate) const POLL_RETRY_MS: i64 = 1_000;

/// Runs one task poll between `Store::begin_poll` and a `rest` that happens on every exit. The run
/// says whether only a write or an event can give the poll work (`Ok(true)`); otherwise it rests
/// for [`POLL_RETRY_MS`] because a row it could not advance may be ready, and a failure rests for
/// the retry delay too. A poll that wrote reports zero whatever its due time, through the change
/// counter, so a row that advanced still runs the tick at its floor.
pub(crate) fn poll_once(
    core: &mut Core,
    poll: &'static str,
    run: impl FnOnce(&mut Core) -> Result<bool>,
) -> Result<()> {
    let Some(seen) = core.store.begin_poll(poll, core.now_ms()) else {
        return Ok(());
    };
    let outcome = run(core);
    let until = match &outcome {
        Ok(true) => i64::MAX,
        _ => core.now_ms().saturating_add(POLL_RETRY_MS),
    };
    core.store.rest(poll, seen, until);
    outcome.map(|_| ())
}

/// Asserts that a task poll that cannot advance its rows rests for the retry delay, not now.
#[cfg(test)]
pub(crate) fn assert_poll_rests_for_retry(core: &Core) {
    let waited = core
        .store
        .next_poll_wait(core.now_ms())
        .expect("the poll rests");
    assert_eq!(
        waited,
        std::time::Duration::from_millis(u64::try_from(POLL_RETRY_MS).unwrap()),
        "a poll that cannot advance rests instead of keeping the floor"
    );
}

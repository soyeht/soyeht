//! Workspace task rules and their persistence boundary.

mod api;
pub(crate) mod blocking;
pub(crate) mod dispatching;
pub mod drift;
pub mod feedback;
mod lifecycle;
pub(crate) mod looping;
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
#[cfg(test)]
pub(crate) use rules::tests::task_fixture;
pub use rules::{
    ExecutorIdentity, apply_block, apply_complete, apply_move, apply_owner_loss, apply_release,
    apply_request_review, apply_review_return, apply_unblock, apply_update, new_loop_config,
    new_task, validate_dependencies, validate_executor,
};

//! Workspace task rules and their persistence boundary.

mod api;
pub mod feedback;
mod lifecycle;
mod phrase;
pub mod ports;
pub mod revision;
mod rules;
pub mod worktree;

pub(crate) use lifecycle::TaskOwnerLoss;
pub use phrase::{PhraseFacts, phrase, review_badge};
#[cfg(test)]
pub(crate) use rules::tests::task_fixture;
pub use rules::{
    ExecutorIdentity, apply_block, apply_complete, apply_move, apply_owner_loss, apply_release,
    apply_request_review, apply_unblock, apply_update, new_loop_config, new_task,
    validate_dependencies, validate_executor,
};

//! Workspace task rules and their persistence boundary.

mod phrase;
pub mod ports;
mod rules;

pub use phrase::{PhraseFacts, phrase, review_badge};
pub use rules::{
    ExecutorIdentity, apply_block, apply_complete, apply_move, apply_release, apply_request_review,
    apply_unblock, apply_update, new_loop_config, new_task, validate_dependencies,
    validate_executor,
};

//! A workspace's tasks, as the Mac shows them (`TaskPresentation`, `TaskReplica`): the board and
//! the list, the cards, the detail and the new-task form. Both clients pass the cases of
//! `contracts/vectors/tasks.json`.

mod presentation;
mod state;
#[cfg(test)]
mod vectors;

pub use presentation::*;
pub use state::TaskUpdate;

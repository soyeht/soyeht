//! Starting a process and ending it together with everything it started.
//!
//! On Unix a child leads its own process group, [`signal_group`] reaches the group and
//! [`kill_tree`] also reaches the descendants that left it. On Windows a child runs in a job
//! object and [`windows::Spawned::kill_tree`] ends the job. Both are the one place the engine
//! knows how to end "a process and what it started". [`detached::spawn_detached`] starts a
//! program that outlives the process that started it. [`end_child`] kills a `std::process::Child`
//! that still runs and reaps it within a bound, the same on every system.

pub mod detached;
mod end;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
pub mod windows;

pub use end::end_child;
#[cfg(unix)]
pub use unix::{GroupSignal, kill_tree, signal_group};

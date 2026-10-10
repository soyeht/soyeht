//! Starting a process and ending it together with everything it started.
//!
//! On Unix a child leads its own process group and [`signal_group`] reaches the group. On
//! Windows a child runs in a job object and [`windows::Spawned::kill_tree`] ends the job. Both are the
//! one place the engine knows how to end "a process and what it started".

#[cfg(unix)]
mod unix;
#[cfg(windows)]
pub mod windows;

#[cfg(unix)]
pub use unix::{GroupSignal, signal_group};

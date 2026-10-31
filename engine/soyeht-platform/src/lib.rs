#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! The operating-system seams of the engine. Every system call that differs between macOS,
//! Linux and Windows lives here, so the crates above it stay free of `cfg` for these concerns:
//!
//! - [`local`]: the endpoint other processes of the same user connect to. A Unix socket on
//!   macOS and Linux; an authenticated loopback connection on Windows.
//! - [`net`]: TCP listeners that no other program can take the port from (Windows).
//! - [`dirs`]: the native folder for a user's application data (Application Support,
//!   `%LOCALAPPDATA%`, XDG).
//! - [`dialog`]: a native message box for a program with no terminal (Windows; no-op elsewhere).
//! - [`diagnostics`]: the report a person pastes into an issue, and the redaction that takes
//!   paths, names and tokens out of it.
//! - [`user`]: who is running the process.
//! - [`log`]: one structured, escaped log line on standard error.
//! - [`shutdown`]: when a stop is requested (the `tokio` feature).
//! - [`process`]: ending a process together with everything it started (a process group on
//!   Unix, a job object on Windows), the Windows launch of a child into one, and the start of a
//!   program that outlives its parent (`process::detached`), and `process::end_child`, which kills a
//!   `std::process::Child` that still runs and reaps it within a bound.
//! - [`fs`]: owner-only files and directories and opens that never follow a link.
//! - [`worktrunk`]: the name of the worktree program, which Windows spells `git-wt`.
//! - [`git`]: whether `git` can run, decided without starting it (the macOS launcher opens
//!   the system installer when the Command Line Tools are missing).
//!
//! The crate has no Soyeht dependencies.

pub mod diagnostics;
pub mod dialog;
pub mod dirs;
pub mod fs;
pub mod git;
pub mod hostname;
pub mod local;
pub mod log;
pub mod net;
pub mod process;
#[cfg(feature = "tokio")]
pub mod shutdown;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod user;
pub mod worktrunk;

/// Tests that start a child and tests that wait for a lock to be released when its file closes
/// exclude each other. On Unix a lock lives as long as its open file description, and a child
/// forked by another test thread holds a copy of it until the child execs, so the lock of a
/// dropped listener would still read as held. Spawning tests share the read side.
#[cfg(test)]
static CHILD_FORKS: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Held by a test that starts a child process, for as long as the child may be forked.
#[cfg(test)]
pub(crate) fn spawning() -> std::sync::RwLockReadGuard<'static, ()> {
    CHILD_FORKS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Held by a test that drops a lock and then expects it to be free: no child is mid-fork.
#[cfg(test)]
pub(crate) fn releasing() -> std::sync::RwLockWriteGuard<'static, ()> {
    CHILD_FORKS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

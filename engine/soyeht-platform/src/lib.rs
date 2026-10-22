#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! The operating-system seams of the engine. Every system call that differs between macOS,
//! Linux and Windows lives here, so the crates above it stay free of `cfg` for these concerns:
//!
//! - [`local`]: the endpoint other processes of the same user connect to. A Unix socket on
//!   macOS and Linux; an authenticated loopback connection on Windows.
//! - [`user`]: who is running the process.
//! - [`log`]: one structured, escaped log line on standard error.
//! - [`shutdown`]: when a stop is requested (the `tokio` feature).
//! - [`process`]: ending a process together with everything it started (a process group on
//!   Unix, a job object on Windows), and the Windows launch of a child into one.
//! - [`fs`]: owner-only files and directories and opens that never follow a link.
//! - [`worktrunk`]: the name of the worktree program, which Windows spells `git-wt`.
//! - [`git`]: whether `git` can run, decided without starting it (the macOS launcher opens
//!   the system installer when the Command Line Tools are missing).
//!
//! The crate has no Soyeht dependencies.

pub mod fs;
pub mod git;
pub mod local;
pub mod log;
pub mod process;
#[cfg(feature = "tokio")]
pub mod shutdown;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod user;
pub mod worktrunk;

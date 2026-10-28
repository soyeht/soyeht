#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

//! The service seam: starting the engine and its supervisor when a person logs in, keeping them
//! running, and letting an installer, an uninstaller or the app register or remove that start
//! (ADR 0114, contract 13).
//!
//! The crate has one public shape on every system. A [`Layout`] says where things are on this
//! machine and builds the [`Definition`] of each [`Component`] from the profile of
//! `contracts/profiles.json`; [`render`] turns a definition into the text of the system's own
//! file (a launchd property list, a systemd user unit, a Task Scheduler task); a [`Manager`]
//! writes that file and asks the system's tool to register, start, stop or remove it. Nothing
//! is `cfg`-gated except [`System::current`]: the three systems' rendering and command
//! sequences compile and are tested everywhere, with the commands behind [`Commands`] so a test
//! runs none.
//!
//! Every name (label, unit, task) comes from the profile, and every one carries "next": a
//! definition whose name or location could be the legacy app's is refused ([`guard`]).

mod commands;
mod definition;
mod error;
pub mod guard;
pub mod host;
mod layout;
mod manage;
mod policy;
mod render;
mod system;

pub use commands::{CommandOutput, Commands, Real};
pub use definition::Definition;
pub use error::{Error, Result};
pub use layout::Layout;
pub use manage::{Manager, Status};
pub use render::{encode_task_file, render};
pub use system::{Component, System};

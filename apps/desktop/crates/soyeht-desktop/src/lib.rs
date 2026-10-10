//! The Soyeht desktop client for Linux, Windows and macOS, drawn with GPUI.
//!
//! `soyeht-client` owns the engine connection and the state; this crate owns the window: the
//! palette tokens (`theme`), the pane shell, the terminal painter and the views.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod app;
mod fonts;
mod pane_shell;
mod terminal_view;
pub mod theme;
mod views;
mod workbench;

pub use app::run;

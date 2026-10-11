//! The Soyeht desktop client for Linux, Windows and macOS, drawn with GPUI.
//!
//! `soyeht-client` owns the engine connection and the state; this crate owns the window: the
//! design tokens (`theme`), the neumorphic surfaces (`neo`), the terminal painter and the views.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod app;
mod assets;
mod composer_field;
mod destination;
mod draft;
mod fonts;
mod neo;
mod open_menu;
mod tab_plan;
mod terminal_view;
pub mod theme;
mod views;
mod window_size;
mod workbench;

pub use app::run;

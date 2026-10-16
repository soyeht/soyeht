//! The Soyeht desktop client for Linux, Windows and macOS, drawn with GPUI.
//!
//! `soyeht-client` owns the engine connection and the state; this crate owns the window: the
//! design tokens (`theme`), the neumorphic surfaces (`neo`), the terminal painter and the views.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod agent_actions;
mod app;
mod assets;
mod attach;
mod composer_field;
mod destination;
mod draft;
mod editor_input;
mod editor_ui;
mod editor_view;
#[cfg(feature = "dev-tools")]
mod fixture;
mod fonts;
#[cfg(feature = "dev-tools")]
mod gallery;
mod graphics;
mod loop_form;
mod neo;
mod open_menu;
mod pane_ui;
mod prefs;
mod switch_flow;
mod tasks_ui;
mod terminal_view;
pub mod theme;
mod views;
mod window_size;
mod workbench;

pub use app::run;
#[cfg(feature = "dev-tools")]
pub use fixture::run as run_fixture;
#[cfg(feature = "dev-tools")]
pub use gallery::run as run_gallery;

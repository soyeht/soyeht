//! The Soyeht desktop client for Linux, Windows and macOS, drawn with GPUI.
//!
//! `soyeht-client` owns the engine connection and the state; this crate owns the window: the
//! design tokens (`theme`), the neumorphic surfaces (`neo`), the terminal painter and the views.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod agent_actions;
mod alerts;
mod app;
mod assets;
mod attach;
mod build_info;
mod composer_field;
mod destination;
mod editor_input;
mod editor_ui;
mod editor_view;
mod file_open;
#[cfg(feature = "dev-tools")]
mod fixture;
mod folder;
mod fonts;
#[cfg(feature = "dev-tools")]
mod gallery;
mod graphics;
mod layout_edit;
mod logging;
mod loop_form;
mod neo;
mod notices;
mod open_menu;
mod os_notify;
mod pane_floors;
mod pane_ui;
mod pointer;
mod popup_fit;
mod prefs;
mod redock;
mod rename;
mod shortcuts;
mod slash_ui;
mod stage;
mod start_flow;
mod switch_flow;
mod tasks_ui;
mod terminal_input;
mod terminal_view;
pub mod theme;
mod update_checks;
mod views;
mod web_page;
mod web_surface;
mod window_size;
mod workbench;
mod writing_flow;
mod writing_style;
mod writing_ui;

pub use app::run;
#[cfg(feature = "dev-tools")]
pub use fixture::run as run_fixture;
#[cfg(feature = "dev-tools")]
pub use gallery::run as run_gallery;

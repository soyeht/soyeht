#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod agent_capture;
pub mod agents;
pub mod auth;
pub mod core;
pub mod core_lock;
pub mod dispatch;
pub mod editor;
pub mod engine_build;
pub mod errors;
mod experiment_build;
mod experiment_export;
mod file_events;
mod frames;
pub mod hooks;
mod immutable_file;
pub mod input;
pub mod languages;
pub mod launch;
pub mod log_rotation;
pub mod model;
pub mod network;
pub mod offload;
mod operation_log;
pub mod outbox;
pub mod pairing;
pub mod pane_observations;
pub mod panes;
pub mod ports;
pub mod push;
pub mod screen;
pub mod server;
pub mod settings;
pub mod skills;
pub mod streak;
pub mod streams;
pub mod tasks;
mod terminal_capture;
mod terminal_drain;
mod terminal_harness;
pub mod terminal_projection;
#[cfg(feature = "test-hooks")]
pub mod test_hooks;
#[cfg(test)]
mod test_panes;
#[cfg(test)]
mod test_paths;
mod tick;
pub mod transport;
pub mod ui_attestation;
pub mod usage;
pub mod vcs;
pub mod web;
mod workspace_layout;
pub mod workspaces;
pub mod writing;

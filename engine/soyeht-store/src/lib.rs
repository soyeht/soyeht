#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod adapter_acknowledgement;
pub mod agents;
pub mod database;
pub use database::{Error, Result};
pub mod domain;
mod due;
pub mod editor;
pub mod editor_tabs;
mod english_values;
pub mod harness_sign_in;
pub mod languages;
pub mod pairing_nodes;
pub mod project_trust;
pub mod push;
mod resting;
pub mod segments;
mod statement_cost;
pub mod tasks;
#[cfg(feature = "test-support")]
pub mod test_support;
pub mod usage;
pub mod web;
pub mod writing;

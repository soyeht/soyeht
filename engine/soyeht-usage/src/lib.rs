//! Read-only subscription-usage readers: one source per provider and method, a planner that
//! walks each provider's fallback chain, and the engine's refresh schedule.
//!
//! The sources, their order and their gates reimplement CodexBar
//! (github.com/steipete/CodexBar, MIT License, © 2026 Peter Steinberger) in Soyeht's engine.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod foreign_db;
pub mod host;
mod model;
mod planner;
mod policy;
mod sources;

pub use model::{Reading, Window};
pub use planner::{PROVIDERS, Planner, RefreshOutcome, TransientFailure};
pub use policy::{ManualRefresh, RefreshPolicy};
pub use sources::{Failure, Trigger};

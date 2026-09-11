#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod agents;
pub mod database;
pub mod domain;
mod english_values;
pub mod segments;
#[cfg(feature = "test-support")]
pub mod test_support;

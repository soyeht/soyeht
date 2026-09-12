//! Read-only subscription-usage normalization and refresh policy.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod error;
mod model;
mod policy;
mod port;
mod providers;
mod registry;

pub use error::{Error, Result};
pub use model::{Reading, Window};
pub use policy::{AutomaticTrigger, ManualRefresh, RefreshPolicy};
pub use port::{SourceRead, UsageSourcePort};
pub use registry::{ReaderRegistry, RefreshOutcome, TransientFailure};

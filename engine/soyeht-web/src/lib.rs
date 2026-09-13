//! Pure, bounded browser policy and data structures.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod command;
mod element;
mod error;
mod local_server;
mod reference;
mod ring;
mod snapshot;
mod url_policy;

pub use command::{DEFAULT_ZOOM_PERCENT, command_response_budget, validate_command, viewport};
pub use element::{ElementCapture, build_element_package};
pub use error::{Error, Result};
pub use local_server::{CwdObservation, LocalServerScan, ServerTitle};
pub use reference::NavigationReferences;
pub use ring::{ConsoleMetadata, ConsoleRing, NetworkMetadata, NetworkRing, RingPage};
pub use snapshot::{SnapshotNodeInput, build_snapshot};
pub use url_policy::NormalizedUrl;

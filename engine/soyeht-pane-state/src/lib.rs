//! Pure, bounded raw-pane observations and replaceable state detection.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod baseline;
mod error;
mod model;

pub use baseline::RegexBaseline;
pub use error::{Error, Result};
pub use model::{
    Confidence, CursorPosition, Detection, Detector, GroundTruthLabel, HarnessIdentity,
    ObservationFrame, RecordedRow,
};

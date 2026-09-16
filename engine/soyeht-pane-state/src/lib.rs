//! Pure, bounded raw-pane observations and replaceable state detection.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod baseline;
mod error;
mod format;
mod metrics;
mod model;

pub use baseline::RegexBaseline;
pub use error::{Error, Result};
pub use format::{
    DATASET_SCHEMA_VERSION, DatasetRow, EngineBuild, MAX_DATASET_ROW_BYTES, MAX_RETAINED_SEGMENTS,
    MAX_SEGMENT_BYTES, ReplayedSegment, SegmentEnd, SegmentMetadata, decode_segment, encode_row,
    segment_schema_version,
};
pub use metrics::{HarnessMetrics, LatencyMetrics, MetricsReport, Rate, StateMetrics, evaluate};
pub use model::{
    Confidence, CursorPosition, Detection, Detector, GroundTruthLabel, HarnessIdentity,
    MAX_FOREGROUND_PROCESS_BYTES, MAX_HARNESS_ID_BYTES, MAX_HARNESS_VERSION_BYTES,
    MAX_PANE_ID_BYTES, MAX_SCREEN_LINE_BYTES, MAX_SCREEN_LINES, MAX_TITLE_BYTES, ObservationFrame,
    ObservationGap, ObservationPaneKind, ObservationReason, RecordedRow,
};

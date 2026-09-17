//! Pure, bounded raw-pane observations and replaceable state detection.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod baseline;
mod error;
mod evaluation;
mod format;
mod metrics;
mod model;

pub use baseline::RegexBaseline;
pub use error::{Error, Result};
pub use evaluation::{
    DatasetCounts, DetectorEvaluation, DetectorRegistry, EvaluationDataset,
    EvaluationOmittedSource, EvaluationReport, EvaluationSource, evaluate_shipped,
};
pub use format::{
    DATASET_SCHEMA_VERSION, DatasetRow, EngineBuild, MAX_DATASET_ROW_BYTES, MAX_RETAINED_SEGMENTS,
    MAX_SEGMENT_BYTES, ReplayedSegment, SegmentEnd, SegmentMetadata, decode_segment, encode_row,
    segment_schema_version,
};
pub use metrics::{
    HarnessMetrics, LatencyMetrics, MIN_GROUND_TRUTH_TRANSITIONS, MIN_RATE_DENOMINATOR,
    MetricsReport, Rate, StateDurationMetrics, StateMetrics, evaluate, evaluate_with_omitted_panes,
};
pub use model::{
    Confidence, CursorPosition, Detection, Detector, GroundTruthChannel, GroundTruthLabel,
    GroundTruthProvenance, HarnessIdentity, MAX_FOREGROUND_PROCESS_BYTES, MAX_HARNESS_ID_BYTES,
    MAX_HARNESS_VERSION_BYTES, MAX_PANE_ID_BYTES, MAX_SCREEN_LINE_BYTES, MAX_SCREEN_LINES,
    MAX_TITLE_BYTES, ObservationFrame, ObservationGap, ObservationPaneKind, ObservationReason,
    RecordedRow,
};

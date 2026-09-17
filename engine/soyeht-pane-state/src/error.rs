use std::fmt::{Display, Formatter};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Empty(&'static str),
    IncompleteRecord,
    InvalidConfidence,
    InvalidDetectorName,
    InvalidDigest,
    InvalidGap,
    InvalidGeneration,
    InvalidHeader,
    InvalidLabelSource,
    InvalidLabelState,
    InvalidScreenLine,
    InvalidSegmentEnd,
    InvalidSequence,
    InvalidTimestamp,
    DuplicateDetectorName,
    Json,
    Limit(&'static str),
    MismatchedProvenance,
    Pattern(String),
    SchemaVersion,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty(field) => write!(formatter, "{field} must not be empty"),
            Self::IncompleteRecord => formatter.write_str("dataset record is incomplete"),
            Self::InvalidConfidence => formatter.write_str("confidence is outside 0 through 1"),
            Self::InvalidDetectorName => formatter.write_str("detector name is not canonical"),
            Self::InvalidDigest => formatter.write_str("engine digest is not canonical SHA-256"),
            Self::InvalidGap => formatter.write_str("observation gap is not a complete interval"),
            Self::InvalidGeneration => formatter.write_str("session generation must be positive"),
            Self::InvalidHeader => formatter.write_str("dataset segment header is invalid"),
            Self::InvalidLabelSource => {
                formatter.write_str("ground truth must come from protocol or hook")
            }
            Self::InvalidLabelState => {
                formatter.write_str("ground truth must describe an observed state")
            }
            Self::InvalidScreenLine => {
                formatter.write_str("screen lines must not contain line breaks")
            }
            Self::InvalidSegmentEnd => {
                formatter.write_str("dataset segment close marker is invalid")
            }
            Self::InvalidSequence => {
                formatter.write_str("dataset sequences must increase monotonically")
            }
            Self::InvalidTimestamp => formatter.write_str("timestamp must be non-negative"),
            Self::DuplicateDetectorName => {
                formatter.write_str("detector registry contains a duplicate name")
            }
            Self::Json => formatter.write_str("dataset record is not valid closed JSON"),
            Self::Limit(field) => write!(formatter, "{field} exceeds its bound"),
            Self::MismatchedProvenance => {
                formatter.write_str("dataset row does not match its segment provenance")
            }
            Self::Pattern(pattern) => write!(formatter, "invalid baseline pattern: {pattern}"),
            Self::SchemaVersion => formatter.write_str("dataset schema version is unsupported"),
        }
    }
}

impl std::error::Error for Error {}

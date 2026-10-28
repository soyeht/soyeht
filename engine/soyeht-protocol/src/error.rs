use std::{fmt, io};

/// The message of the `ptyd_unavailable` error a request gets while the engine has no link to
/// the supervisor of the terminals, and what a client says while that lasts.
pub const SUPERVISOR_DISCONNECTED: &str = "Supervisor disconnected";

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    UnknownFrameType(u8),
    InvalidFrameLength { kind: u8, length: usize },
    FrameLengthOverflow,
    MissingTerminalHeader,
    MalformedTerminalHeader,
    MissingAttachmentHeader,
    MalformedAttachmentHeader,
    InvalidAttachmentLimit,
    InvalidAttachmentId,
    InvalidAttachmentSize,
    AttachmentTooLarge,
    InterleavedAttachment,
    UnknownAttachment,
    DuplicateAttachment,
    IncompleteAttachment,
    JsonFrameRequired,
    NdjsonStreamDiscontinuity { expected: u64, actual: u64 },
    NdjsonOffsetExhausted,
    InvalidEmbeddedSchema,
    UnknownMethod,
    InvalidParameters,
    EngineChallengeExpected,
    UnexpectedHandshake,
    IncompleteSnapshot,
    InconsistentSnapshotPages,
    SnapshotSectionNotArray,
    HomeMissing,
    UnknownProfile,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => write!(formatter, "JSON: {error}"),
            Self::UnknownFrameType(kind) => write!(formatter, "unknown frame type {kind}"),
            Self::InvalidFrameLength { kind, length } => {
                write!(formatter, "invalid length {length} for frame type {kind}")
            }
            Self::FrameLengthOverflow => formatter.write_str("frame length overflow"),
            Self::MissingTerminalHeader => formatter.write_str("terminal header is missing"),
            Self::MalformedTerminalHeader => formatter.write_str("terminal header is malformed"),
            Self::MissingAttachmentHeader => formatter.write_str("attachment header is missing"),
            Self::MalformedAttachmentHeader => {
                formatter.write_str("attachment header is malformed")
            }
            Self::InvalidAttachmentLimit => formatter.write_str("attachment limit is invalid"),
            Self::InvalidAttachmentId => formatter.write_str("attachment id is invalid"),
            Self::InvalidAttachmentSize => formatter.write_str("attachment size is invalid"),
            Self::AttachmentTooLarge => formatter.write_str("attachment exceeds its byte limit"),
            Self::InterleavedAttachment => formatter.write_str("attachment chunks are interleaved"),
            Self::UnknownAttachment => formatter.write_str("attachment reference is unknown"),
            Self::DuplicateAttachment => formatter.write_str("attachment is duplicated"),
            Self::IncompleteAttachment => {
                formatter.write_str("attachment byte count is incomplete")
            }
            Self::JsonFrameRequired => formatter.write_str("JSON frame required"),
            Self::NdjsonStreamDiscontinuity { expected, actual } => write!(
                formatter,
                "NDJSON stream offset {actual} does not match expected offset {expected}"
            ),
            Self::NdjsonOffsetExhausted => formatter.write_str("NDJSON stream offset exhausted"),
            Self::InvalidEmbeddedSchema => formatter.write_str("compiled schema is invalid"),
            Self::UnknownMethod => formatter.write_str("unknown method"),
            Self::InvalidParameters => formatter.write_str("invalid parameters"),
            Self::EngineChallengeExpected => {
                formatter.write_str("engine did not send an authentication challenge")
            }
            Self::UnexpectedHandshake => formatter.write_str("unexpected engine handshake"),
            Self::IncompleteSnapshot => formatter.write_str("incomplete snapshot"),
            Self::InconsistentSnapshotPages => {
                formatter.write_str("snapshot pages are inconsistent")
            }
            Self::SnapshotSectionNotArray => {
                formatter.write_str("snapshot section must be an array")
            }
            Self::HomeMissing => formatter.write_str("the home directory is unknown"),
            Self::UnknownProfile => formatter.write_str("the profile name is not known"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

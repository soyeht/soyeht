use std::{fmt, io};

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
    InvalidEmbeddedSchema,
    UnknownMethod,
    InvalidParameters,
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
            Self::InvalidEmbeddedSchema => formatter.write_str("compiled schema is invalid"),
            Self::UnknownMethod => formatter.write_str("unknown method"),
            Self::InvalidParameters => formatter.write_str("invalid parameters"),
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

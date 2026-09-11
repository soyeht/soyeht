use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Protocol(soyeht_protocol::Error),
    Json(serde_json::Error),
    Base64(base64::DecodeError),
    Remote { kind: String, message: String },
    InvalidWelcome,
    InvalidResponse,
    ZeroRetentionLimit,
    OutputOffsetExhausted,
    AckAhead,
    CursorAhead,
    PendingRequestNotFound,
    InvalidPendingOffset,
    PoisonedLock(&'static str),
    AlreadyRunning,
    SocketPathNotSocket,
    JsonRequestRequired,
    ConnectionSuperseded,
    RegistryInstanceMissing,
    InvalidBase64Input,
    InvalidSpawnParameters,
    EnvironmentVariableNotAllowed,
    InvalidTerminalSize,
    MissingGuardianResponse,
    GuardianFailed(String),
    InvalidGuardianResponse,
    GuardianStdinMissing,
    GuardianStdoutMissing,
    InvalidGuardianDescriptor,
    GuardianNotSessionLeader,
    EmptyCommand,
    IncompatibleRegistryVersion,
    SupervisorGenerationExhausted,
    HomeMissing,
    InvalidGuardianFd,
    InvalidArguments,
}

impl Error {
    pub fn wire_kind(&self) -> &'static str {
        match self {
            Self::AckAhead => "ack_ahead",
            Self::CursorAhead => "cursor_ahead",
            Self::PendingRequestNotFound => "not_found",
            Self::InvalidPendingOffset => "invalid_offset",
            Self::OutputOffsetExhausted => "output_offset_overflow",
            _ => "operation_failed",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Protocol(error) => write!(formatter, "protocol: {error}"),
            Self::Json(error) => write!(formatter, "JSON: {error}"),
            Self::Base64(error) => write!(formatter, "base64: {error}"),
            Self::Remote { kind, message } => write!(formatter, "{kind}: {message}"),
            Self::InvalidWelcome => formatter.write_str("invalid ptyd welcome"),
            Self::InvalidResponse => formatter.write_str("invalid ptyd response"),
            Self::ZeroRetentionLimit => formatter.write_str("retention limit is zero"),
            Self::OutputOffsetExhausted => formatter.write_str("output offset exhausted"),
            Self::AckAhead => formatter.write_str("acknowledgement is ahead of output"),
            Self::CursorAhead => formatter.write_str("cursor is ahead of output"),
            Self::PendingRequestNotFound => formatter.write_str("pending request not found"),
            Self::InvalidPendingOffset => formatter.write_str("invalid pending request offset"),
            Self::PoisonedLock(resource) => write!(formatter, "{resource} lock is poisoned"),
            Self::AlreadyRunning => formatter.write_str("ptyd is already running"),
            Self::SocketPathNotSocket => formatter.write_str("socket path is not a socket"),
            Self::JsonRequestRequired => formatter.write_str("JSON request required"),
            Self::ConnectionSuperseded => formatter.write_str("connection superseded"),
            Self::RegistryInstanceMissing => formatter.write_str("registry instance disappeared"),
            Self::InvalidBase64Input => formatter.write_str("invalid base64 input"),
            Self::InvalidSpawnParameters => formatter.write_str("invalid spawn parameters"),
            Self::EnvironmentVariableNotAllowed => {
                formatter.write_str("environment variable is not allowed")
            }
            Self::InvalidTerminalSize => formatter.write_str("invalid terminal size"),
            Self::MissingGuardianResponse => formatter.write_str("missing pipe guardian response"),
            Self::GuardianFailed(message) => write!(formatter, "pipe guardian failed: {message}"),
            Self::InvalidGuardianResponse => formatter.write_str("invalid pipe guardian response"),
            Self::GuardianStdinMissing => {
                formatter.write_str("pipe guardian stdin was not created")
            }
            Self::GuardianStdoutMissing => {
                formatter.write_str("pipe guardian stdout was not created")
            }
            Self::InvalidGuardianDescriptor => {
                formatter.write_str("invalid pipe guardian descriptor")
            }
            Self::GuardianNotSessionLeader => {
                formatter.write_str("pipe guardian must own its session")
            }
            Self::EmptyCommand => formatter.write_str("empty command"),
            Self::IncompatibleRegistryVersion => {
                formatter.write_str("incompatible registry version")
            }
            Self::SupervisorGenerationExhausted => {
                formatter.write_str("supervisor generation exhausted")
            }
            Self::HomeMissing => formatter.write_str("HOME is required"),
            Self::InvalidGuardianFd => formatter.write_str("invalid pipe guardian descriptor"),
            Self::InvalidArguments => {
                formatter.write_str("usage: soyeht-ptyd [--data-dir <isolated-directory>]")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Protocol(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Base64(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<soyeht_protocol::Error> for Error {
    fn from(error: soyeht_protocol::Error) -> Self {
        Self::Protocol(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<base64::DecodeError> for Error {
    fn from(error: base64::DecodeError) -> Self {
        Self::Base64(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

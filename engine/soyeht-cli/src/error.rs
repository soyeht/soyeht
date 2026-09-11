use soyeht_protocol::RpcError;
use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Protocol(soyeht_protocol::Error),
    Json(serde_json::Error),
    Remote(RpcError),
    EngineChallengeExpected,
    UnexpectedHandshake,
    UnexpectedFrame,
    RequestIdExhausted,
    AttemptsExhausted,
    IncompleteSnapshot,
    InconsistentSnapshotPages,
    SnapshotSectionNotArray,
    ResponseMissingResult,
    InvalidArguments(String),
    HomeMissing,
    MissingArgument,
    UnsupportedProfile,
    MissingDataDirectory,
    EngineUnavailable(Box<Error>),
}

impl Error {
    pub fn io_kind(&self) -> Option<io::ErrorKind> {
        match self {
            Self::Io(error) => Some(error.kind()),
            Self::Protocol(soyeht_protocol::Error::Io(error)) => Some(error.kind()),
            Self::EngineUnavailable(error) => error.io_kind(),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Protocol(error) => write!(formatter, "protocol: {error}"),
            Self::Json(error) => write!(formatter, "JSON: {error}"),
            Self::Remote(error) => {
                write!(formatter, "remote error {}: {}", error.code, error.message)
            }
            Self::EngineChallengeExpected => {
                formatter.write_str("engine did not send an authentication challenge")
            }
            Self::UnexpectedHandshake => formatter.write_str("unexpected engine handshake"),
            Self::UnexpectedFrame => {
                formatter.write_str("engine closed or sent an unexpected binary frame")
            }
            Self::RequestIdExhausted => formatter.write_str("request identifiers exhausted"),
            Self::AttemptsExhausted => formatter.write_str("request attempts exhausted"),
            Self::IncompleteSnapshot => formatter.write_str("incomplete snapshot"),
            Self::InconsistentSnapshotPages => {
                formatter.write_str("snapshot pages are inconsistent")
            }
            Self::SnapshotSectionNotArray => {
                formatter.write_str("snapshot section must be an array")
            }
            Self::ResponseMissingResult => formatter.write_str("response has no result"),
            Self::InvalidArguments(message) => formatter.write_str(message),
            Self::HomeMissing => formatter.write_str("HOME is required"),
            Self::MissingArgument => formatter.write_str("missing command-line argument"),
            Self::UnsupportedProfile => {
                formatter.write_str("only the new dev profile is available")
            }
            Self::MissingDataDirectory => formatter.write_str("missing data directory"),
            Self::EngineUnavailable(error) => {
                write!(formatter, "cannot connect to the new Dev engine: {error}")
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
            Self::EngineUnavailable(error) => Some(error),
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

pub type Result<T> = std::result::Result<T, Error>;

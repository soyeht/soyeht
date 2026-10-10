use soyeht_protocol::RpcError;
use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    Protocol(soyeht_protocol::Error),
    Remote(RpcError),
    InvalidArguments(&'static str),
    UnexpectedFrame,
    IncompleteLine,
    RequestIdExhausted,
    ResponseMissingResult,
    ResponseMissingAttachment,
    UnknownTool,
    ToolForbidden,
    TaskContextRequired,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => write!(formatter, "JSON: {error}"),
            Self::Protocol(error) => write!(formatter, "protocol: {error}"),
            Self::Remote(error) => {
                write!(formatter, "remote error {}: {}", error.code, error.message)
            }
            Self::InvalidArguments(message) => formatter.write_str(message),
            Self::UnexpectedFrame => formatter.write_str("unexpected engine frame"),
            Self::IncompleteLine => formatter.write_str("incomplete JSON-RPC line"),
            Self::RequestIdExhausted => formatter.write_str("request identifiers exhausted"),
            Self::ResponseMissingResult => formatter.write_str("engine response has no result"),
            Self::ResponseMissingAttachment => {
                formatter.write_str("engine response has no screenshot attachment")
            }
            Self::UnknownTool => formatter.write_str("unknown task tool"),
            Self::ToolForbidden => formatter.write_str("task tool is unavailable for this role"),
            Self::TaskContextRequired => formatter.write_str("an active task is required"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Protocol(error) => Some(error),
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

impl From<soyeht_protocol::Error> for Error {
    fn from(error: soyeht_protocol::Error) -> Self {
        Self::Protocol(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

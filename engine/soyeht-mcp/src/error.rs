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

/// A failure the service cannot give the model logs as its class, never its message.
impl soyeht_platform::log::Failure for Error {
    fn failure_kind(&self) -> String {
        match self {
            Self::Io(error) => format!("io:{:?}", error.kind()),
            Self::Json(_) => "json".to_owned(),
            Self::Protocol(_) => "protocol".to_owned(),
            Self::Remote(_) => "remote".to_owned(),
            Self::UnexpectedFrame => "unexpected_frame".to_owned(),
            Self::IncompleteLine => "incomplete_line".to_owned(),
            Self::RequestIdExhausted => "request_id_exhausted".to_owned(),
            Self::ResponseMissingResult => "response_missing_result".to_owned(),
            Self::ResponseMissingAttachment => "response_missing_attachment".to_owned(),
            Self::InvalidArguments(_)
            | Self::UnknownTool
            | Self::ToolForbidden
            | Self::TaskContextRequired => "invalid_request".to_owned(),
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

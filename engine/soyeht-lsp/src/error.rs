use std::fmt;

#[derive(Debug)]
pub enum Error {
    HeaderTooLarge,
    MessageTooLarge,
    InvalidHeader,
    MissingContentLength,
    DuplicateContentLength,
    InvalidJson(serde_json::Error),
    InvalidMessage,
    InvalidResponseId,
    UnmatchedResponse,
    Remote { code: i64, message: String },
    RequestLimit,
    RequestIdExhausted,
    InvalidPayload(&'static str),
    Transport(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeaderTooLarge => formatter.write_str("LSP header exceeds its byte limit"),
            Self::MessageTooLarge => formatter.write_str("LSP message exceeds its byte limit"),
            Self::InvalidHeader => formatter.write_str("LSP header is malformed"),
            Self::MissingContentLength => formatter.write_str("LSP message has no content length"),
            Self::DuplicateContentLength => {
                formatter.write_str("LSP message repeats content length")
            }
            Self::InvalidJson(error) => write!(formatter, "invalid LSP JSON: {error}"),
            Self::InvalidMessage => formatter.write_str("LSP JSON-RPC message is malformed"),
            Self::InvalidResponseId => formatter.write_str("LSP response ID is malformed"),
            Self::UnmatchedResponse => formatter.write_str("LSP response has no pending request"),
            Self::Remote { code, message } => write!(formatter, "LSP error {code}: {message}"),
            Self::RequestLimit => formatter.write_str("too many LSP requests are pending"),
            Self::RequestIdExhausted => formatter.write_str("LSP request ID space is exhausted"),
            Self::InvalidPayload(field) => write!(formatter, "LSP payload has an invalid {field}"),
            Self::Transport(message) => write!(formatter, "LSP transport failed: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidJson(error) => Some(error),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidJson(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

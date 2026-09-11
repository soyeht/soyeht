use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    Acp(soyeht_acp::Error),
    MissingLoadPolicy,
    MissingVersionLabel,
    UnknownOption(String),
    InvalidStateDirectory,
    UnknownLoadPolicy,
    InvalidSessionId,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => write!(formatter, "JSON: {error}"),
            Self::Acp(error) => write!(formatter, "ACP: {error}"),
            Self::MissingLoadPolicy => formatter.write_str("missing load policy"),
            Self::MissingVersionLabel => formatter.write_str("missing version label"),
            Self::UnknownOption(option) => write!(formatter, "unknown test-agent option: {option}"),
            Self::InvalidStateDirectory => {
                formatter.write_str("--state-dir needs an absolute disposable directory")
            }
            Self::UnknownLoadPolicy => formatter.write_str("unknown load policy"),
            Self::InvalidSessionId => formatter.write_str("invalid session ID"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Acp(error) => Some(error),
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

impl From<soyeht_acp::Error> for Error {
    fn from(error: soyeht_acp::Error) -> Self {
        Self::Acp(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

use std::{fmt, io};

pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong between the desktop client and the engine.
#[derive(Debug)]
pub enum Error {
    /// A frame or request the shared protocol crate refused.
    Protocol(soyeht_protocol::Error),
    Io(io::Error),
    Json(serde_json::Error),
    /// The engine answered a request with a JSON-RPC error.
    Remote(soyeht_protocol::RpcError),
    /// The engine did not answer a request in time.
    Timeout(String),
    /// The connection is gone; the supervisor reconnects.
    Disconnected,
    /// The engine broke a handshake or stream rule.
    Invalid(&'static str),
    /// The command queue is full: the engine answers more slowly than the user acts.
    Busy,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => write!(formatter, "JSON: {error}"),
            Self::Remote(error) => formatter.write_str(&error.message),
            Self::Timeout(method) => {
                write!(formatter, "The engine did not answer {method} in time.")
            }
            Self::Disconnected => formatter.write_str("Connection interrupted. Reconnecting..."),
            Self::Invalid(reason) => {
                write!(formatter, "The engine sent an invalid message: {reason}.")
            }
            Self::Busy => formatter.write_str("The engine is busy. The last action was not sent."),
        }
    }
}

impl std::error::Error for Error {}

/// A failure logs as its class, never as its message: an I/O error's text can carry a path and
/// an engine's own words can carry anything. An engine refusal logs its protocol error kind.
impl soyeht_platform::log::Failure for Error {
    fn failure_kind(&self) -> String {
        match self {
            Self::Protocol(_) => "protocol".to_owned(),
            Self::Io(error) => format!("io:{:?}", error.kind()),
            Self::Json(_) => "json".to_owned(),
            Self::Remote(error) => match serde_json::to_value(&error.data.kind) {
                Ok(serde_json::Value::String(kind)) => kind,
                _ => "remote".to_owned(),
            },
            Self::Timeout(_) => "timeout".to_owned(),
            Self::Disconnected => "disconnected".to_owned(),
            Self::Invalid(_) => "invalid".to_owned(),
            Self::Busy => "busy".to_owned(),
        }
    }
}

impl From<soyeht_protocol::Error> for Error {
    fn from(error: soyeht_protocol::Error) -> Self {
        Self::Protocol(error)
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

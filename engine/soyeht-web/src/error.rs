use soyeht_protocol::ErrorKind;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    EmptyUrl,
    InvalidUrl,
    UnsupportedScheme,
    MissingHost,
    PortScanTooLarge,
    InvalidPortRecord,
    InvalidSnapshotNode,
    ReferenceExhausted,
    StaleReference,
    ElementLimit,
    InvalidElement,
    PayloadTooLarge,
    InvalidConsoleEntry,
    InvalidNetworkEntry,
    InvalidViewport,
    InvalidWait,
    HumanModeUnavailable,
    InvalidCommand,
    SequenceExhausted,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyUrl => "URL is empty",
            Self::InvalidUrl => "URL is not valid",
            Self::UnsupportedScheme => "URL scheme is not allowed",
            Self::MissingHost => "URL has no host",
            Self::PortScanTooLarge => "port scan output exceeds its byte budget",
            Self::InvalidPortRecord => "port scan output contains an invalid record",
            Self::InvalidSnapshotNode => "snapshot node is not valid",
            Self::ReferenceExhausted => "element reference space is exhausted",
            Self::StaleReference => "element reference belongs to another navigation",
            Self::ElementLimit => "element selection exceeds its count limit",
            Self::InvalidElement => "element package is not valid",
            Self::PayloadTooLarge => "web payload exceeds its byte budget",
            Self::InvalidConsoleEntry => "console metadata is not valid",
            Self::InvalidNetworkEntry => "network metadata is not valid",
            Self::InvalidViewport => "web viewport is not valid",
            Self::InvalidWait => "web wait is not valid",
            Self::HumanModeUnavailable => "human mode is unavailable on this host",
            Self::InvalidCommand => "web command is not valid",
            Self::SequenceExhausted => "web metadata sequence is exhausted",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for Error {}

/// The protocol kind the engine presents for a web failure. The mapping lives beside the
/// failures it names, so a new one cannot be added without deciding what a client sees.
impl From<&Error> for ErrorKind {
    fn from(error: &Error) -> Self {
        match error {
            Error::EmptyUrl
            | Error::InvalidUrl
            | Error::UnsupportedScheme
            | Error::MissingHost
            | Error::InvalidViewport
            | Error::InvalidWait
            | Error::InvalidCommand => ErrorKind::NavigationRefused,
            Error::StaleReference => ErrorKind::StaleReference,
            Error::HumanModeUnavailable => ErrorKind::WebHostUnavailable,
            Error::PortScanTooLarge
            | Error::InvalidPortRecord
            | Error::InvalidSnapshotNode
            | Error::ReferenceExhausted
            | Error::ElementLimit
            | Error::InvalidElement
            | Error::PayloadTooLarge
            | Error::InvalidConsoleEntry
            | Error::InvalidNetworkEntry
            | Error::SequenceExhausted => ErrorKind::InternalError,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

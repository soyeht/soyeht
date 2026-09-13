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

pub type Result<T> = std::result::Result<T, Error>;

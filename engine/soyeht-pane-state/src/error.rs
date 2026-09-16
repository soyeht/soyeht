use std::fmt::{Display, Formatter};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Empty(&'static str),
    InvalidConfidence,
    InvalidLabelSource,
    InvalidLabelState,
    InvalidScreenLine,
    InvalidTimestamp,
    Limit(&'static str),
    Pattern(String),
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty(field) => write!(formatter, "{field} must not be empty"),
            Self::InvalidConfidence => formatter.write_str("confidence is outside 0 through 1"),
            Self::InvalidLabelSource => {
                formatter.write_str("ground truth must come from protocol or hook")
            }
            Self::InvalidLabelState => {
                formatter.write_str("ground truth must describe an observed state")
            }
            Self::InvalidScreenLine => {
                formatter.write_str("screen lines must not contain line breaks")
            }
            Self::InvalidTimestamp => formatter.write_str("timestamp must be non-negative"),
            Self::Limit(field) => write!(formatter, "{field} exceeds its bound"),
            Self::Pattern(pattern) => write!(formatter, "invalid baseline pattern: {pattern}"),
        }
    }
}

impl std::error::Error for Error {}

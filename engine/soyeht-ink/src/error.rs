use soyeht_protocol::{ErrorKind, InkUnsupportedReason};
use std::fmt;

/// Why handwriting was not read. No variant holds the writing or the text it reads: an error is
/// answered to the person who asked and may be logged by its kind, and ink is neither (ADR 0158).
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The request is malformed: no strokes, an empty stroke, no language, a language tag that
    /// is not one.
    Invalid(&'static str),
    /// The request is larger than a limit of the contract allows; the text names the limit.
    LimitExceeded(&'static str),
    /// This machine has no way to read handwriting, and why.
    Unsupported(InkUnsupportedReason),
    /// There is a recognizer, but it reads none of the languages asked for.
    LanguageUnavailable,
    /// The recognizer failed on writing it could have read; the text says at which step.
    Failed(&'static str),
    /// Recognition was asked for by a thread that holds the engine's Core lock (ADR 0079).
    UnderLock(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(what) | Self::LimitExceeded(what) | Self::Failed(what) => {
                formatter.write_str(what)
            }
            Self::Unsupported(InkUnsupportedReason::NoOsSupport) => {
                formatter.write_str("This system has no handwriting recognizer")
            }
            Self::Unsupported(InkUnsupportedReason::LanguagePackMissing) => {
                formatter.write_str("The handwriting recognizer's language data is not installed")
            }
            Self::Unsupported(InkUnsupportedReason::ModelNotInstalled) => {
                formatter.write_str("The handwriting recognizer's model is not installed")
            }
            Self::LanguageUnavailable => {
                formatter.write_str("The handwriting recognizer reads none of the languages asked")
            }
            Self::UnderLock(holder) => {
                write!(formatter, "Handwriting is not read while {holder} is held")
            }
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// The reason a client reads in `data.detail` of an `ink_unsupported` error.
    pub fn unsupported_reason(&self) -> Option<&InkUnsupportedReason> {
        match self {
            Self::Unsupported(reason) => Some(reason),
            _ => None,
        }
    }
}

/// The protocol kind the engine presents for an ink failure. The mapping lives beside the
/// failures it names, so a new one cannot be added without deciding what a client sees.
impl From<&Error> for ErrorKind {
    fn from(error: &Error) -> Self {
        match error {
            Error::Invalid(_) => ErrorKind::InvalidParams,
            Error::LimitExceeded(_) => ErrorKind::InkLimitExceeded,
            Error::Unsupported(_) => ErrorKind::InkUnsupported,
            Error::LanguageUnavailable => ErrorKind::InkLanguageUnavailable,
            Error::Failed(_) => ErrorKind::InkRecognitionFailed,
            Error::UnderLock(_) => ErrorKind::InternalError,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

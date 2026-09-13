use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidRepository,
    InvalidPath,
    CommandUnavailable,
    CommandFailed,
    InvalidUtf8,
    OutputTooLarge,
    MalformedBlame,
    NumericOverflow,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRepository => "source-control repository is unavailable",
            Self::InvalidPath => "source-control path is outside the repository",
            Self::CommandUnavailable => "source-control command is unavailable",
            Self::CommandFailed => "source-control command failed",
            Self::InvalidUtf8 => "source-control output is not UTF-8",
            Self::OutputTooLarge => "source-control result exceeds its byte budget",
            Self::MalformedBlame => "source-control blame output is malformed",
            Self::NumericOverflow => "source-control line number exceeds the supported range",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

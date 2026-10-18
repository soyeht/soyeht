use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidRepository,
    InvalidPath,
    CommandUnavailable,
    /// `git` was not started: it would open the system installer, or it is not installed.
    GitUnavailable,
    CommandFailed,
    TimedOut,
    InvalidUtf8,
    OutputTooLarge,
    MalformedBlame,
    HunkNotFound,
    InvalidHunkRange,
    NumericOverflow,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRepository => "source-control repository is unavailable",
            Self::InvalidPath => "source-control path is outside the repository",
            Self::CommandUnavailable => "source-control command is unavailable",
            Self::GitUnavailable => soyeht_process::GIT_UNAVAILABLE_MESSAGE,
            Self::CommandFailed => "source-control command failed",
            Self::TimedOut => "source-control command timed out and was stopped",
            Self::InvalidUtf8 => "source-control output is not UTF-8",
            Self::OutputTooLarge => "source-control result exceeds its byte budget",
            Self::MalformedBlame => "source-control blame output is malformed",
            Self::HunkNotFound => "source-control hunk is unavailable",
            Self::InvalidHunkRange => "source-control hunk range is invalid",
            Self::NumericOverflow => "source-control line number exceeds the supported range",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

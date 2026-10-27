use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Why a web view could not do what was asked. Kinds only: a message can carry a page's address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// This build has no system web view.
    Unsupported,
    /// The system web view is not on this machine, or has no display to draw for.
    Unavailable(&'static str),
    /// The view or the thread that serves it has ended.
    Closed,
    /// The engine did not answer in time.
    Timeout,
    /// The engine or the page refused: a script that threw, a capture it could not make.
    Page(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("the web view is not available on this system"),
            Self::Unavailable(why) => write!(f, "the web view is not available: {why}"),
            Self::Closed => f.write_str("the web view is closed"),
            Self::Timeout => f.write_str("the web view did not answer in time"),
            Self::Page(what) => write!(f, "the page could not be read: {what}"),
        }
    }
}

impl std::error::Error for Error {}

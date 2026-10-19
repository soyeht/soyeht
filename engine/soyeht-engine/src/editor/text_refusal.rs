//! Why a readable file cannot be edited as text.

use std::{fmt, io};

/// It travels inside an `InvalidData` error, so that `read_disk` answers with its own typed code
/// and a client can show the reason in the pane instead of a generic failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TextRefusal {
    NotText,
    TooLarge,
}

impl fmt::Display for TextRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotText => "file is not UTF-8 text",
            Self::TooLarge => "file exceeds the editor size limit",
        })
    }
}

impl std::error::Error for TextRefusal {}

impl From<TextRefusal> for io::Error {
    fn from(refusal: TextRefusal) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, refusal)
    }
}

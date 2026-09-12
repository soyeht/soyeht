use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidPosition,
    InvalidRange,
    OverlappingEdits,
    EditTooLarge,
    ViewportTooLarge,
    ParserLanguage,
    ParserStopped,
    EmptySearch,
    SearchPattern,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidPosition => "text position is outside the document",
            Self::InvalidRange => "text range is reversed or splits a scalar value",
            Self::OverlappingEdits => "text edits overlap",
            Self::EditTooLarge => "text edit exceeds the protocol limit",
            Self::ViewportTooLarge => "one editor line exceeds the viewport byte budget",
            Self::ParserLanguage => "syntax parser refused its language",
            Self::ParserStopped => "syntax parser did not return a tree",
            Self::EmptySearch => "search query is empty",
            Self::SearchPattern => "search pattern could not be compiled",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

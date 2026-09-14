use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    DocumentTooLarge,
    InvalidPosition,
    InvalidRange,
    EmptySelection,
    MissingFormatValue,
    SlideLimit,
    SceneLimit,
    ElementLimit,
    VariantLimit,
    VariantTooLarge,
    VariantNotFound,
    CannotRemoveOnlyVariant,
    InvalidVariantSelection,
    BeatLimit,
    BeatNotFound,
    SceneNotFound,
    LinkLimit,
    InvalidBeat,
    InvalidSectionPath,
    SectionLimit,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::DocumentTooLarge => "writing document exceeds its byte budget",
            Self::InvalidPosition => "writing position is outside the document",
            Self::InvalidRange => "writing range is reversed or splits a scalar",
            Self::EmptySelection => "writing format requires selected text",
            Self::MissingFormatValue => "writing format requires a value",
            Self::SlideLimit => "presentation exceeds its slide limit",
            Self::SceneLimit => "screenplay exceeds its scene limit",
            Self::ElementLimit => "screenplay exceeds its element limit",
            Self::VariantLimit => "line exceeds its variant limit",
            Self::VariantTooLarge => "line variant exceeds its byte budget",
            Self::VariantNotFound => "line variant does not exist",
            Self::CannotRemoveOnlyVariant => "the only line variant cannot be removed",
            Self::InvalidVariantSelection => "selected line variant is invalid",
            Self::BeatLimit => "beat board exceeds its card limit",
            Self::BeatNotFound => "beat card does not exist",
            Self::SceneNotFound => "linked screenplay scene does not exist",
            Self::LinkLimit => "beat board exceeds its scene-link limit",
            Self::InvalidBeat => "beat card is invalid",
            Self::InvalidSectionPath => "document path is outside its writing section",
            Self::SectionLimit => "writing section exceeds its entry limit",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

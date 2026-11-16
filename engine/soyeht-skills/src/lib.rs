//! Soyeht skills: folders holding a `SKILL.md` in the open Agent Skills
//! format, kept in plain directories that people, the app and agents all
//! write with ordinary file tools. This crate reads and edits one such
//! directory; the engine decides which directories exist and how each
//! harness receives them.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod home;
mod manifest;
mod store;

pub use manifest::{Manifest, parse, render, valid_name};
use std::fmt;
pub use store::{Entry, Store};

/// File name of a skill's manifest inside its folder.
pub const MANIFEST: &str = "SKILL.md";

/// Why a skill folder cannot be used. Listing keeps such folders, so the
/// person sees what to fix instead of a skill that silently vanished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    MissingManifest,
    Unreadable,
    TooLarge,
    NotText,
    MissingFrontmatter,
    UnterminatedFrontmatter,
    MissingName,
    InvalidName,
    NameMismatch,
    MissingDescription,
    DescriptionTooLong,
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingManifest => "The folder has no SKILL.md.",
            Self::Unreadable => "SKILL.md cannot be read.",
            Self::TooLarge => "SKILL.md is larger than the 256 KiB limit.",
            Self::NotText => "SKILL.md is not UTF-8 text.",
            Self::MissingFrontmatter => {
                "SKILL.md must start with a frontmatter block between --- lines."
            }
            Self::UnterminatedFrontmatter => "The frontmatter has no closing --- line.",
            Self::MissingName => "The frontmatter has no name.",
            Self::InvalidName => {
                "The name must be 1 to 64 lowercase letters, digits or single hyphens."
            }
            Self::NameMismatch => "The name must match the folder name.",
            Self::MissingDescription => "The frontmatter has no description.",
            Self::DescriptionTooLong => "The description is longer than 1024 characters.",
        })
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidName,
    NotFound,
    AlreadyExists,
    StaleRevision,
    Invalid(Problem),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::InvalidName => return Problem::InvalidName.fmt(formatter),
            Self::Invalid(problem) => return problem.fmt(formatter),
            Self::Io(error) => return write!(formatter, "Skill storage failed: {error}"),
            Self::NotFound => "Skill not found",
            Self::AlreadyExists => "A skill with this name already exists",
            Self::StaleRevision => "The skill changed on disk; reload it before saving",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

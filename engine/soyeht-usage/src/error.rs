use soyeht_protocol::{UsageProvider, UsageSource};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    ReaderMissing { provider: UsageProvider },
    ReaderHasNoSource { provider: UsageProvider },
    CacheNotPopulated { source: UsageSource },
    InvalidResponse { source: UsageSource },
    InvalidWindow { source: UsageSource },
    UnverifiedResponse { source: UsageSource },
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReaderMissing { provider } => {
                write!(formatter, "usage reader is missing for {provider:?}")
            }
            Self::ReaderHasNoSource { provider } => {
                write!(formatter, "usage reader has no source for {provider:?}")
            }
            Self::CacheNotPopulated { source } => {
                write!(formatter, "usage cache is not populated for {source:?}")
            }
            Self::InvalidResponse { source } => {
                write!(formatter, "invalid response from {source:?}")
            }
            Self::InvalidWindow { source } => {
                write!(formatter, "invalid usage window from {source:?}")
            }
            Self::UnverifiedResponse { source } => {
                write!(formatter, "unverified response shape from {source:?}")
            }
        }
    }
}

impl std::error::Error for Error {}

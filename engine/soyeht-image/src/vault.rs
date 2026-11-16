//! The person's own key for a cloud provider, kept on the computer that builds the avatar (ADR
//! 0166, section 2). One owner-only file per provider in an owner-only folder of the data
//! directory. The key is never printed, never logged and never put on a command line: it is read
//! here and written into the request's configuration, which reaches `curl` on standard input.

use crate::error::{Error, Result};
use soyeht_platform::fs::{WriteMode, ensure_private_dir, write_atomic};
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

const FOLDER: &str = "image-keys";
/// Longest key accepted; real keys are a hundred characters or so.
const MAX_KEY_BYTES: usize = 1000;

/// A key. It prints as nothing.
pub struct Secret(String);

impl Secret {
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(..)")
    }
}

pub struct Vault {
    directory: PathBuf,
}

impl Vault {
    pub fn new(data_directory: &Path) -> Self {
        Self {
            directory: data_directory.join(FOLDER),
        }
    }

    /// The file of a provider. The id comes from the catalogue, and is checked again here so a
    /// value from anywhere else can never leave the folder.
    fn file(&self, provider: &str) -> Result<PathBuf> {
        let plain = !provider.is_empty()
            && !provider.starts_with('.')
            && !provider.contains("..")
            && provider.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
            });
        if !plain {
            return Err(Error::UnknownProvider(provider.to_owned()));
        }
        Ok(self.directory.join(provider))
    }

    /// The stored key of `provider`, if there is one.
    pub fn read(&self, provider: &str) -> Result<Option<Secret>> {
        let text = present(fs::read_to_string(self.file(provider)?))?;
        Ok(text
            .map(|text| Secret(text.trim().to_owned()))
            .filter(|key| !key.0.is_empty()))
    }

    /// Stores `key` for `provider`. A key is printable characters with no space.
    pub fn set(&self, provider: &str, key: &str) -> Result<()> {
        let key = key.trim();
        if key.is_empty() || key.len() > MAX_KEY_BYTES {
            return Err(Error::InvalidKey("empty or too long"));
        }
        if !key.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(Error::InvalidKey("only printable characters with no space"));
        }
        let file = self.file(provider)?;
        ensure_private_dir(&self.directory).map_err(io::Error::from)?;
        write_atomic(&file, key.as_bytes(), WriteMode::OwnerOnly)?;
        Ok(())
    }

    /// Forgets the key of `provider`; true when there was one.
    pub fn clear(&self, provider: &str) -> Result<bool> {
        Ok(present(fs::remove_file(self.file(provider)?))?.is_some())
    }
}

/// `Some` of what the operation gave, or `None` when the file is not there.
fn present<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    result.map(Some).or_else(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        }
    })
}

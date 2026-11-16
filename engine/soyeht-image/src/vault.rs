//! The person's own key for a cloud provider, kept on the computer that builds the avatar (ADR
//! 0166, section 2). A key belongs to one provider **and one address**: it is stored with the host
//! it was given for, and a call to another host does not find it, so a key given for one service
//! is never sent to a service the person did not give it for.
//!
//! One owner-only file per provider and host (`image-keys/<provider>/<digest of host>`) in
//! owner-only folders of the data directory. The first line of the file is the host, the second
//! the key. The key is never printed, never logged and never put on a command line.

use crate::error::{Error, Result};
use sha2::{Digest, Sha256};
use soyeht_platform::fs::{WriteMode, ensure_private_dir, write_atomic};
use soyeht_platform::secret::Secret;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const FOLDER: &str = "image-keys";
/// Longest key accepted; real keys are a hundred characters or so.
const MAX_KEY_BYTES: usize = 1000;
/// Characters of the host's digest in a file name.
const NAME_DIGEST: usize = 16;

/// The one rule for a key, on the way in and on the way out of the file: what a header can hold
/// without escaping (`Secret::is_usable`), within a size a key does not exceed, so it can only
/// ever be one header value.
fn secret(text: &str) -> Result<Secret> {
    let key = Secret::new(text.trim());
    if key.expose().len() > MAX_KEY_BYTES || !key.is_usable() {
        return Err(Error::InvalidKey(
            "empty, too long, or not printable characters with no space, quote or backslash",
        ));
    }
    Ok(key)
}

/// The host and the key of a stored file.
fn parse(text: &str) -> Option<(&str, &str)> {
    text.split_once('\n')
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

    /// The folder of a provider. The id comes from the catalogue, and is checked again here so a
    /// value from anywhere else can never leave the folder.
    fn folder(&self, provider: &str) -> Result<PathBuf> {
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

    fn file(&self, provider: &str, host: &str) -> Result<PathBuf> {
        let digest = format!("{:x}", Sha256::digest(host.as_bytes()));
        Ok(self.folder(provider)?.join(&digest[..NAME_DIGEST]))
    }

    /// Stores `key` for `provider` at `host` (`Choice::key_host`).
    pub fn set(&self, provider: &str, host: &str, key: &str) -> Result<()> {
        let key = secret(key)?;
        let file = self.file(provider, host)?;
        let folder = self.folder(provider)?;
        ensure_private_dir(&self.directory).map_err(io::Error::from)?;
        ensure_private_dir(&folder).map_err(io::Error::from)?;
        let text = format!("{host}\n{}\n", key.expose());
        write_atomic(&file, text.as_bytes(), WriteMode::OwnerOnly)?;
        Ok(())
    }

    /// The key stored for `provider` at `host`. A provider with keys for other addresses only is
    /// told apart from one with none.
    pub fn key_for(&self, provider: &str, host: &str) -> Result<Secret> {
        match present(fs::read_to_string(self.file(provider, host)?))? {
            Some(text) => match parse(&text) {
                Some((stored, key)) if stored == host => secret(key),
                _ => Err(Error::InvalidKey("the stored key file is damaged")),
            },
            None if self.hosts(provider)?.is_empty() => Err(Error::MissingKey),
            None => Err(Error::KeyForAnotherAddress),
        }
    }

    /// The addresses `provider` has a key for, sorted.
    pub fn hosts(&self, provider: &str) -> Result<Vec<String>> {
        let Some(entries) = present(fs::read_dir(self.folder(provider)?))? else {
            return Ok(Vec::new());
        };
        let mut hosts = Vec::new();
        for entry in entries {
            let text = fs::read_to_string(entry?.path())?;
            if let Some((host, _)) = parse(&text) {
                hosts.push(host.to_owned());
            }
        }
        hosts.sort();
        Ok(hosts)
    }

    /// Forgets the key of `provider` at `host`, or every key of it when no host is given; how many
    /// keys went. It needs no catalogue entry, so a key of an entry that has left can still go.
    pub fn clear(&self, provider: &str, host: Option<&str>) -> Result<usize> {
        match host {
            Some(host) => Ok(usize::from(
                present(fs::remove_file(self.file(provider, host)?))?.is_some(),
            )),
            None => {
                let count = self.hosts(provider)?.len();
                present(fs::remove_dir_all(self.folder(provider)?))?;
                Ok(count)
            }
        }
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

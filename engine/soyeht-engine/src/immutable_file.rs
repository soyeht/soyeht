//! Write-once files named by their content. A file is prepared beside its
//! target, synchronized, then published with an exclusive hard link, so a
//! reader never sees partial bytes and a retry never overwrites anything.

use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};
use uuid::Uuid;

/// The lowercase hexadecimal SHA-256 that names content-addressed files.
pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

/// Publishes `bytes` at `target` in an existing directory. An existing regular
/// file with the same bytes is accepted, as after an interrupted retry; any
/// other existing entry fails. The temporary file is removed on every path.
pub(crate) fn publish(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no directory"))?;
    let temporary = parent.join(format!(".soyeht-publish-{}", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let published = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        match fs::hard_link(&temporary, target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                verify_existing(target, bytes)
            }
            Err(error) => Err(error),
        }
    })();
    let _ = fs::remove_file(&temporary);
    published?;
    fs::File::open(parent)?.sync_all()
}

fn verify_existing(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let metadata = fs::symlink_metadata(target)?;
    let expected_len = u64::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "content is too large"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != expected_len
        || fs::read(target)? != bytes
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "target already holds different content",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_is_exclusive_idempotent_and_leaves_no_temporary_file() {
        let directory = std::env::temp_dir().join(format!("immutable-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let target = directory.join("asset.bin");
        publish(&target, b"bytes").unwrap();
        publish(&target, b"bytes").unwrap();
        assert_eq!(
            publish(&target, b"other").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&target).unwrap(), b"bytes");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
}

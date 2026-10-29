//! Write-once files named by their content. A file is prepared beside its
//! target, synchronized, then published with an exclusive hard link, so a
//! reader never sees partial bytes and a retry never overwrites anything.

use sha2::{Digest, Sha256};
use soyeht_platform::fs::{WriteMode, is_temporary_name, publish_new};
use std::{fs, io, path::Path, time::Duration};

/// A temporary file of `publish` that nobody has touched for this long is the leftover of a
/// crash: no write takes that long.
const STALE_AFTER: Duration = Duration::from_secs(3600);

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

/// Publishes `bytes` at `target` in an existing directory, by `soyeht_platform::fs::publish_new`
/// (an exclusive hard link, with the rename for file systems that have none). An existing
/// regular file with the same bytes is accepted, as after an interrupted retry; any other
/// existing entry fails. The leftovers of a crash in this folder are swept first.
pub(crate) fn publish(target: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = target.parent() {
        sweep_stale(parent);
    }
    publish_new(target, bytes, WriteMode::KeepPermissions)
}

/// Removes the temporary files of this folder that a crash left behind: the platform's own, and
/// untouched for an hour, so one that another write is using now is never taken. Best effort
/// (a failure is logged by kind and the file stays for the next time).
fn sweep_stale(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = is_temporary_name(&entry.file_name())
            && matches!(
                entry.metadata().and_then(|meta| meta.modified()).map(|time| time.elapsed()),
                Ok(Ok(age)) if age > STALE_AFTER
            );
        if stale && let Err(error) = fs::remove_file(entry.path()) {
            soyeht_platform::log::failure("engine.temporary_sweep_failed", &error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("immutable-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        directory
    }

    #[test]
    fn publication_is_exclusive_idempotent_and_leaves_no_temporary_file() {
        let directory = scratch();
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

    #[test]
    fn a_publish_sweeps_the_temporary_files_a_crash_left_and_only_those() {
        let directory = scratch();
        let old = std::time::SystemTime::now() - Duration::from_secs(2 * 3600);
        let leftover = directory.join(format!("{}crashed", soyeht_platform::fs::TEMPORARY_PREFIX));
        let fresh = directory.join(format!("{}in-use", soyeht_platform::fs::TEMPORARY_PREFIX));
        let mine = directory.join(".soyeht-publisher-notes");
        for path in [&leftover, &fresh, &mine] {
            fs::write(path, "x").unwrap();
        }
        for path in [&leftover, &mine] {
            fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        publish(&directory.join("a.bin"), b"a").unwrap();
        assert!(!leftover.exists());
        assert!(fresh.exists(), "a file another write may be using is kept");
        assert!(mine.exists(), "only the platform's own names are taken");
        fs::remove_dir_all(directory).unwrap();
    }
}

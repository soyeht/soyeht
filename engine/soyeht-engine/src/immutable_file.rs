//! Write-once files named by their content. A file is prepared beside its
//! target, synchronized, then published with an exclusive hard link, so a
//! reader never sees partial bytes and a retry never overwrites anything.

use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    time::Duration,
};
use uuid::Uuid;

/// Prefix of the file `publish` writes next to its target before it links it.
const PUBLISH_TEMPORARY_PREFIX: &str = ".soyeht-publish-";

/// True for the engine's own temporary files, which exist only mid-write and belong to the engine,
/// not to the project: no listing, search or file event may show one. One rule for both kinds: an
/// atomic save's (`soyeht_platform::fs`) and a publish's.
pub(crate) fn is_engine_temporary(name: &std::ffi::OsStr) -> bool {
    soyeht_platform::fs::is_temporary_name(name)
        || name
            .to_str()
            .is_some_and(|name| name.starts_with(PUBLISH_TEMPORARY_PREFIX))
}

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

/// Publishes `bytes` at `target` in an existing directory. An existing regular
/// file with the same bytes is accepted, as after an interrupted retry; any
/// other existing entry fails. The temporary file is removed on every path.
///
/// Where the link fails for a reason other than an existing target (FAT and exFAT volumes, some
/// network shares: the error is not one kind across systems) the temporary
/// file is renamed to the target when nothing is there. That is atomic too, so no file with
/// missing bytes is ever visible, even after a crash; what it gives up is the exclusive link, so a
/// program that creates the target in the instant between the check and the rename is
/// overwritten, which a plain save of the editor does as well.
pub(crate) fn publish(target: &Path, bytes: &[u8]) -> io::Result<()> {
    publish_linking(target, bytes, |from, to| fs::hard_link(from, to))
}

fn publish_linking(
    target: &Path,
    bytes: &[u8],
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no directory"))?;
    sweep_stale(parent);
    let temporary = parent.join(format!("{PUBLISH_TEMPORARY_PREFIX}{}", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let published = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        match link(&temporary, target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                verify_existing(target, bytes)
            }
            // Any other failure of the link: what a file system without hard links answers is not
            // one kind (not permitted on Linux FAT, errno 45 on macOS FAT and exFAT, an
            // uncategorized code on Windows), so the rename tries, and a real cause fails it too.
            Err(error) => {
                soyeht_platform::log::failure("engine.publish_link_failed", &error);
                rename_if_absent(&temporary, target, bytes)
            }
        }
    })();
    // A temporary file already renamed to the target is gone; the failure of the removal of one
    // that is not leaves it for the sweep of the next publish in this folder.
    let _ = fs::remove_file(&temporary);
    published?;
    soyeht_platform::fs::sync_directory(parent)
}

fn rename_if_absent(temporary: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    match fs::symlink_metadata(target) {
        Ok(_) => verify_existing(target, bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::rename(temporary, target),
        Err(error) => Err(error),
    }
}

/// Removes the temporary files of this folder that a crash left behind: the engine's own, and
/// untouched for an hour, so one that another write is using now is never taken. Best effort
/// (a failure is logged by kind and the file stays for the next time).
fn sweep_stale(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = is_engine_temporary(&entry.file_name())
            && matches!(
                entry.metadata().and_then(|meta| meta.modified()).map(|time| time.elapsed()),
                Ok(Ok(age)) if age > STALE_AFTER
            );
        if stale && let Err(error) = fs::remove_file(entry.path()) {
            soyeht_platform::log::failure("engine.temporary_sweep_failed", &error);
        }
    }
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

    fn scratch() -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("immutable-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        directory
    }

    /// What a file system without hard links answers, by kind and as the systems give it: not
    /// permitted (Linux FAT), not supported (macOS FAT and exFAT answer errno 45, which Rust does
    /// not categorize), another device, and a code of no kind at all (Windows).
    fn link_failures() -> Vec<io::Error> {
        let mut failures: Vec<io::Error> = [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Unsupported,
            io::ErrorKind::CrossesDevices,
            io::ErrorKind::TimedOut,
        ]
        .into_iter()
        .map(io::Error::from)
        .collect();
        failures.push(io::Error::from_raw_os_error(45));
        failures.push(io::Error::other("no kind"));
        failures
    }

    #[test]
    fn a_link_that_fails_for_any_reason_gets_the_file_by_rename_with_all_its_bytes() {
        for failure in link_failures() {
            let description = failure.to_string();
            let directory = scratch();
            let target = directory.join("doc.txt");
            publish_linking(&target, b"all of it", |_, _| Err(failure)).unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"all of it", "{description}");
            assert_eq!(fs::read_dir(&directory).unwrap().count(), 1, "no temporary");
            // The same bytes are accepted again, other content is refused, as with a link.
            let again = || Err(io::Error::from_raw_os_error(45));
            publish_linking(&target, b"all of it", |_, _| again()).unwrap();
            let refused = publish_linking(&target, b"other", |_, _| again()).unwrap_err();
            assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read(&target).unwrap(), b"all of it");
            fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn a_real_cause_fails_the_rename_too_and_publishes_nothing() {
        let directory = scratch();
        let target = directory.join("doc.txt");
        // The link fails and the temporary file is gone, as with a disk that fails.
        let error = publish_linking(&target, b"x", |from, _| {
            fs::remove_file(from).unwrap();
            Err(io::ErrorKind::TimedOut.into())
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_publish_sweeps_the_temporary_files_a_crash_left_and_only_those() {
        let directory = scratch();
        let old = std::time::SystemTime::now() - Duration::from_secs(2 * 3600);
        let leftover = directory.join(format!("{PUBLISH_TEMPORARY_PREFIX}crashed"));
        let save_leftover =
            directory.join(format!("{}crashed", soyeht_platform::fs::TEMPORARY_PREFIX));
        let fresh = directory.join(format!("{PUBLISH_TEMPORARY_PREFIX}in-use"));
        let mine = directory.join(".soyeht-publisher-notes");
        for path in [&leftover, &save_leftover, &fresh, &mine] {
            fs::write(path, "x").unwrap();
        }
        for path in [&leftover, &save_leftover, &mine] {
            fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        publish(&directory.join("a.bin"), b"a").unwrap();
        assert!(!leftover.exists() && !save_leftover.exists());
        assert!(fresh.exists(), "a file another write may be using is kept");
        assert!(mine.exists(), "only the engine's own names are taken");
        fs::remove_dir_all(directory).unwrap();
    }
}

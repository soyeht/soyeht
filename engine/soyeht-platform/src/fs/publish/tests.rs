use super::*;
use crate::fs::{is_temporary_name, set_private_dir, symlink};
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "soyeht-platform-publish-{name}-{}-{}",
        std::process::id(),
        temporary_name()
    ));
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn entries(directory: &Path) -> usize {
    fs::read_dir(directory).unwrap().count()
}

#[test]
fn publication_is_exclusive_idempotent_and_leaves_no_temporary_file() {
    let directory = scratch("exclusive");
    let target = directory.join("asset.bin");
    publish_new(&target, b"bytes", WriteMode::KeepPermissions).unwrap();
    publish_new(&target, b"bytes", WriteMode::KeepPermissions).unwrap();
    for other in [&b"other"[..], b"bytes and more", b""] {
        let error = publish_new(&target, other, WriteMode::KeepPermissions).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }
    assert_eq!(fs::read(&target).unwrap(), b"bytes");
    assert_eq!(entries(&directory), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn an_owner_only_publication_is_owner_only_and_an_open_file_is_not_accepted_in_its_place() {
    let directory = scratch("owner-only");
    set_private_dir(&directory).unwrap();
    let target = directory.join("export.jsonl");
    publish_new(&target, b"private\n", WriteMode::OwnerOnly).unwrap();
    assert!(file_is_owner_only(&File::open(&target).unwrap()).unwrap());
    publish_new(&target, b"private\n", WriteMode::OwnerOnly).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        let error = publish_new(&target, b"private\n", WriteMode::OwnerOnly).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        // The same bytes are fine where the mode asks nothing about who reads them.
        publish_new(&target, b"private\n", WriteMode::KeepPermissions).unwrap();
    }
    assert_eq!(entries(&directory), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_link_in_the_place_of_the_target_is_refused_and_never_written_through() {
    let directory = scratch("link");
    let outside = directory.join("outside");
    fs::write(&outside, b"keep").unwrap();
    let target = directory.join("export.jsonl");
    if symlink(&outside, &target).is_err() {
        // Windows without Developer Mode cannot make a link.
        fs::remove_dir_all(directory).unwrap();
        return;
    }
    // Even with the bytes the link points at.
    for bytes in [&b"replace"[..], b"keep"] {
        let error = publish_new(&target, bytes, WriteMode::OwnerOnly).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    }
    assert_eq!(fs::read(&outside).unwrap(), b"keep");
    assert_eq!(entries(&directory), 2);
    fs::remove_dir_all(directory).unwrap();
}

/// What a file system without hard links answers, by kind and as the systems give it: not
/// permitted (Linux FAT), not supported (macOS FAT and exFAT answer errno 45, which Rust does not
/// categorize), another device, and a code of no kind at all (Windows).
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
    for mode in [WriteMode::KeepPermissions, WriteMode::OwnerOnly] {
        for failure in link_failures() {
            let description = failure.to_string();
            let directory = scratch("rename");
            set_private_dir(&directory).unwrap();
            let target = directory.join("doc.txt");
            publish_linking(&target, b"all of it", mode, |_, _| Err(failure)).unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"all of it", "{description}");
            assert_eq!(entries(&directory), 1, "no temporary");
            // The same bytes are accepted again, other content is refused, as with a link.
            let again = || Err(io::Error::from_raw_os_error(45));
            publish_linking(&target, b"all of it", mode, |_, _| again()).unwrap();
            let refused = publish_linking(&target, b"other", mode, |_, _| again()).unwrap_err();
            assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read(&target).unwrap(), b"all of it");
            fs::remove_dir_all(directory).unwrap();
        }
    }
}

#[test]
fn a_real_cause_fails_the_rename_too_and_publishes_nothing() {
    let directory = scratch("real-cause");
    let target = directory.join("doc.txt");
    // The link fails and the temporary file is gone, as with a disk that fails.
    let error = publish_linking(&target, b"x", WriteMode::KeepPermissions, |from, _| {
        fs::remove_file(from).unwrap();
        Err(io::ErrorKind::TimedOut.into())
    })
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(entries(&directory), 0);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn the_temporary_file_has_the_name_a_listing_hides() {
    let directory = scratch("name");
    let target = directory.join("doc.txt");
    let seen = std::cell::RefCell::new(Vec::new());
    publish_linking(&target, b"x", WriteMode::KeepPermissions, |from, to| {
        seen.borrow_mut().push(from.file_name().unwrap().to_owned());
        fs::hard_link(from, to)
    })
    .unwrap();
    let seen = seen.into_inner();
    assert_eq!(seen.len(), 1);
    assert!(is_temporary_name(&seen[0]));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_target_without_a_file_name_is_refused() {
    let error = publish_new(Path::new("/"), b"x", WriteMode::OwnerOnly).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

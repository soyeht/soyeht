//! `fs::read_regular_under`: a file of a folder someone else filled is read as itself, never
//! through a link and never by waiting on a pipe.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::fs::{read_regular_under, symlink};
use std::{fs, io, path::Path};

fn folder(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("soyeht-regular-{name}-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(root.join("inside")).unwrap();
    fs::write(root.join("file.txt"), b"hello").unwrap();
    fs::write(root.join("inside/deep.txt"), b"deep").unwrap();
    root
}

fn kind(result: io::Result<Vec<u8>>) -> Option<io::ErrorKind> {
    result.err().map(|error| error.kind())
}

#[test]
fn a_regular_file_is_read_whole_and_one_over_the_limit_is_not() {
    let root = folder("plain");
    assert_eq!(
        read_regular_under(&root, Path::new("file.txt"), 5).unwrap(),
        b"hello"
    );
    assert_eq!(
        read_regular_under(&root, Path::new("inside/deep.txt"), 100).unwrap(),
        b"deep"
    );
    assert_eq!(
        kind(read_regular_under(&root, Path::new("file.txt"), 4)),
        Some(io::ErrorKind::InvalidData)
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_path_that_is_not_plain_is_refused() {
    let root = folder("plain-path");
    for bad in [
        "",
        "../file.txt",
        "/etc/hostname",
        "inside/../file.txt",
        ".",
    ] {
        assert_eq!(
            kind(read_regular_under(&root, Path::new(bad), 100)),
            Some(io::ErrorKind::InvalidInput),
            "{bad:?}"
        );
    }
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn a_link_in_the_name_or_in_a_folder_of_the_path_is_refused() {
    let root = folder("links");
    symlink(root.join("file.txt"), root.join("link.txt")).unwrap();
    symlink(root.join("inside"), root.join("linked-dir")).unwrap();
    assert_eq!(
        kind(read_regular_under(&root, Path::new("link.txt"), 100)),
        Some(io::ErrorKind::InvalidInput)
    );
    assert_eq!(
        kind(read_regular_under(
            &root,
            Path::new("linked-dir/deep.txt"),
            100
        )),
        Some(io::ErrorKind::InvalidInput)
    );
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn a_pipe_is_refused_and_does_not_block() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let root = folder("pipe");
    let pipe = CString::new(root.join("pipe").as_os_str().as_bytes()).unwrap();
    // SAFETY: `pipe` is a valid, NUL-terminated path that outlives the call.
    assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
    assert_eq!(
        kind(read_regular_under(&root, Path::new("pipe"), 100)),
        Some(io::ErrorKind::InvalidData)
    );
    fs::remove_dir_all(&root).unwrap();
}

//! Fuzz targets for the file helpers every crate relies on: canonical paths, and the atomic and
//! exclusive writers. Whatever the name or the bytes, a write lands where it was asked, leaves
//! no temporary file, is owner-only when asked, and never writes through a link or over a file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use soyeht_platform::fs::{self, TEMPORARY_PREFIX, WriteMode, is_temporary_name};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[path = "../../soyeht-protocol/tests/fuzz_support/mod.rs"]
mod fuzz_support;
use fuzz_support::{Corpus, Rng, fuzz_bytes};

/// Cases per write target: each write is synced to disk, which a loaded disk makes slow.
const WRITE_CASES: u64 = 100;
const CASES: u64 = 3_000;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!("soyeht-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A single file name from the bytes: letters, digits and a few characters that are awkward in
/// a path, never a separator, a dot-only name or the prefix of a temporary file.
fn file_name(bytes: &[u8]) -> OsString {
    const ALPHABET: &[u8] = b"abcXYZ019 -_.~$%'()[]{};,+=@#!&";
    let mut name: String = bytes
        .iter()
        .take(40)
        .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
        .collect();
    if name.is_empty() || name.chars().all(|c| c == '.') || name.starts_with(TEMPORARY_PREFIX) {
        name.insert_str(0, "n-");
    }
    // Windows drops a trailing dot or space from a name.
    name.push('x');
    name.into()
}

fn entries(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn assert_no_temporary(directory: &Path) {
    let names = entries(directory);
    assert!(
        !names.iter().any(|name| is_temporary_name(name.as_ref())),
        "{names:?}"
    );
}

#[test]
fn an_atomic_write_lands_exactly_leaves_no_temporary_and_is_owner_only() {
    let corpus = Corpus {
        seeds: &[b"name", b"secret bytes", b""],
        tokens: &[b"\n", b"\0", b"a"],
        max_len: 90,
    };
    let scratch = Scratch::new("fuzz-atomic");
    fuzz_bytes("platform-write-atomic", WRITE_CASES, &corpus, |input| {
        let split = input
            .len()
            .min(1 + usize::from(input.first().copied().unwrap_or(0)) % 40);
        let name = file_name(&input[..split]);
        let contents = &input[split..];
        let target = scratch.0.join(&name);
        fs::write_atomic(&target, contents, WriteMode::OwnerOnly).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), contents);
        let file = std::fs::File::open(&target).unwrap();
        assert!(fs::file_is_owner_only(&file).unwrap());
        // A second write replaces the bytes; the folder holds the one file, never a leftover.
        fs::write_atomic(&target, b"second", WriteMode::OwnerOnly).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");
        assert_no_temporary(&scratch.0);
        std::fs::remove_file(&target).unwrap();
    });
}

#[cfg(unix)]
#[test]
fn an_atomic_write_replaces_a_link_and_never_writes_through_it() {
    let scratch = Scratch::new("fuzz-link");
    let victim = scratch.0.join("victim");
    std::fs::write(&victim, "private").unwrap();
    let corpus = Corpus {
        seeds: &[b"a", b"b"],
        tokens: &[],
        max_len: 40,
    };
    fuzz_bytes(
        "platform-write-through-link",
        WRITE_CASES / 2,
        &corpus,
        |input| {
            let link = scratch.0.join(file_name(input));
            fs::symlink(&victim, &link).unwrap();
            fs::write_atomic(&link, input, WriteMode::OwnerOnly).unwrap();
            assert_eq!(std::fs::read_to_string(&victim).unwrap(), "private");
            assert!(
                !std::fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            std::fs::remove_file(&link).unwrap();
            // A link that is exclusive-published over is refused, not followed.
            fs::symlink(&victim, &link).unwrap();
            let error = fs::publish_new(&link, input, WriteMode::OwnerOnly).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
            assert_eq!(std::fs::read_to_string(&victim).unwrap(), "private");
            std::fs::remove_file(&link).unwrap();
        },
    );
}

#[test]
fn an_exclusive_publish_never_replaces_what_is_there() {
    let scratch = Scratch::new("fuzz-publish");
    let corpus = Corpus {
        seeds: &[b"first", b"second"],
        tokens: &[],
        max_len: 60,
    };
    fuzz_bytes("platform-publish-new", WRITE_CASES, &corpus, |input| {
        let target = scratch.0.join(file_name(input));
        fs::publish_new(&target, input, WriteMode::OwnerOnly).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), input);
        // The same bytes again are accepted; other bytes are refused and the file is untouched.
        fs::publish_new(&target, input, WriteMode::OwnerOnly).unwrap();
        let mut other = input.to_vec();
        other.push(b'!');
        let error = fs::publish_new(&target, &other, WriteMode::OwnerOnly).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&target).unwrap(), input);
        assert_no_temporary(&scratch.0);
        std::fs::remove_file(&target).unwrap();
    });
}

#[test]
fn canonical_paths_are_absolute_existing_and_idempotent_for_any_spelling() {
    let scratch = Scratch::new("fuzz-canonical");
    let root = scratch.0.clone();
    std::fs::create_dir_all(root.join("a/b")).unwrap();
    std::fs::write(root.join("a/b/file"), "x").unwrap();
    #[cfg(unix)]
    {
        fs::symlink(root.join("a"), root.join("link")).unwrap();
        fs::symlink(root.join("missing"), root.join("dangling")).unwrap();
        fs::symlink(root.join("loop2"), root.join("loop1")).unwrap();
        fs::symlink(root.join("loop1"), root.join("loop2")).unwrap();
    }
    let pieces: [&str; 14] = [
        "a", "b", "file", "link", "dangling", "loop1", "..", ".", "", "missing", "A", "a/b",
        "../a", "link/..",
    ];
    let seeds: &[&[u8]] = &[b"\x00\x01\x02", b"\x03\x06\x00"];
    let corpus = Corpus {
        seeds,
        tokens: &[],
        max_len: 12,
    };
    fuzz_bytes("platform-canonicalize", CASES, &corpus, |input| {
        let mut rng = Rng::new(input.iter().fold(0u64, |a, b| a * 31 + u64::from(*b)));
        let mut path = root.clone();
        for byte in input {
            path.push(pieces[usize::from(*byte) % pieces.len()]);
            if rng.chance(9) {
                path.push("..");
            }
        }
        if let Ok(canonical) = fs::canonicalize(&path) {
            assert!(canonical.is_absolute(), "{path:?} gave {canonical:?}");
            assert!(canonical.exists());
            assert!(
                !canonical.to_string_lossy().starts_with(r"\\?\"),
                "{canonical:?}"
            );
            assert_eq!(fs::canonicalize(&canonical).unwrap(), canonical);
        }
    });
}

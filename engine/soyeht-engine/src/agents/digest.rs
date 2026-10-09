//! SHA-256 of harness executables, reused until the file changes.
//!
//! Hashing reads the whole file, which takes seconds for a native binary on a busy
//! host. Catalogue reads need the digest to match live sessions and repeat for every
//! request, so a digest is kept until its file's stamp changes. A file touched within
//! the last couple of seconds is never kept (git's "racily clean" rule): on a filesystem
//! with coarse timestamps a second write inside the same tick would leave the stamp
//! unchanged.

use crate::errors::{self, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

/// What identifies an executable's bytes without reading them. A write changes the
/// modification time and the change time (which user space cannot set), and a
/// replacement changes the inode, so an unchanged stamp means unchanged content.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl FileStamp {
    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.size(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }

    /// True once the file has been untouched for longer than `RACY_WINDOW_SECONDS` at
    /// `now`, so a later write cannot hide inside the timestamp's resolution.
    fn settled(&self, now: i64) -> bool {
        now.saturating_sub(self.modified.0.max(self.changed.0)) > RACY_WINDOW_SECONDS
    }
}

type DigestCache = Mutex<HashMap<PathBuf, (FileStamp, String)>>;

// Registry entries and custom harnesses are few; the bound only keeps a runaway
// profile from growing the cache for the life of the engine.
const DIGEST_CACHE_ENTRIES: usize = 256;

/// The coarsest timestamp tick of the filesystems that hold harness executables (FAT
/// and some network mounts keep two-second times).
const RACY_WINDOW_SECONDS: i64 = 2;

pub(super) fn executable_digest(path: &Path) -> Result<String> {
    static DIGESTS: OnceLock<DigestCache> = OnceLock::new();
    // A clock before the epoch counts as time zero, which settles nothing: no caching.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        });
    cached_digest(
        DIGESTS.get_or_init(DigestCache::default),
        path,
        now,
        hash_file,
    )
}

fn cached_digest(
    cache: &DigestCache,
    path: &Path,
    now: i64,
    hash: impl FnOnce(&Path) -> Result<String>,
) -> Result<String> {
    // Stamp first: a write that lands while hashing leaves a stale stamp behind,
    // so the next request sees a different one and hashes again.
    let stamp = FileStamp::of(&fs::metadata(path).map_err(errors::io)?);
    if let Ok(entries) = cache.lock()
        && let Some((cached, digest)) = entries.get(path)
        && *cached == stamp
    {
        return Ok(digest.clone());
    }
    let digest = hash(path)?;
    if !stamp.settled(now) {
        return Ok(digest);
    }
    // A poisoned cache only costs the next request a fresh hash.
    if let Ok(mut entries) = cache.lock() {
        if entries.len() >= DIGEST_CACHE_ENTRIES {
            entries.clear();
        }
        entries.insert(path.to_owned(), (stamp, digest.clone()));
    }
    Ok(digest)
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(errors::io)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(errors::io)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::super::registry::tests::Fixture;
    use super::*;
    use std::cell::Cell;

    /// A clock far past any file time a test creates: every stamp counts as settled.
    const LONG_AFTER: i64 = 1 << 40;

    fn counted_hash(hashes: &Cell<usize>) -> impl Fn(&Path) -> Result<String> + '_ {
        |path| {
            hashes.set(hashes.get() + 1);
            hash_file(path)
        }
    }

    #[test]
    fn executable_digest_is_reused_until_the_file_changes() {
        let fixture = Fixture::new();
        let path = fixture.executable("agent");
        let cache = DigestCache::default();
        let hashes = Cell::new(0);
        let first = cached_digest(&cache, &path, LONG_AFTER, counted_hash(&hashes)).unwrap();
        assert_eq!(
            cached_digest(&cache, &path, LONG_AFTER, counted_hash(&hashes)).unwrap(),
            first
        );
        assert_eq!(hashes.get(), 1, "an untouched executable is hashed once");
        assert_eq!(first, hash_file(&path).unwrap());

        // Same length and same modification time: only the change time betrays the write.
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, b"#!/bin/sh\nexit 1\n").unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let rewritten = cached_digest(&cache, &path, LONG_AFTER, counted_hash(&hashes)).unwrap();
        assert_ne!(
            rewritten, first,
            "rewritten bytes must not keep the old digest"
        );
        assert_eq!(hashes.get(), 2);

        let replacement = fixture.0.join("replacement");
        fs::write(&replacement, b"#!/bin/sh\nexit 2\n").unwrap();
        fs::rename(&replacement, &path).unwrap();
        let replaced = cached_digest(&cache, &path, LONG_AFTER, counted_hash(&hashes)).unwrap();
        assert_ne!(replaced, rewritten, "a replaced file must be hashed again");
        assert_eq!(hashes.get(), 3);
    }

    #[test]
    fn a_missing_executable_is_an_error_not_a_cached_digest() {
        let fixture = Fixture::new();
        let cache = DigestCache::default();
        let path = fixture.0.join("late");
        assert!(cached_digest(&cache, &path, LONG_AFTER, hash_file).is_err());
        assert_eq!(fixture.executable("late"), path);
        assert!(cached_digest(&cache, &path, LONG_AFTER, hash_file).is_ok());
    }

    #[test]
    fn a_file_touched_within_the_racy_window_is_never_cached() {
        let fixture = Fixture::new();
        let path = fixture.executable("agent");
        let touched = fs::metadata(&path).unwrap().ctime();
        let cache = DigestCache::default();
        let hashes = Cell::new(0);
        for now in [touched, touched + RACY_WINDOW_SECONDS] {
            for _ in 0..2 {
                cached_digest(&cache, &path, now, counted_hash(&hashes)).unwrap();
            }
        }
        assert_eq!(hashes.get(), 4, "a racily clean file is hashed every time");
        assert!(cache.lock().unwrap().is_empty());

        let settled = touched + RACY_WINDOW_SECONDS + 1;
        for _ in 0..2 {
            cached_digest(&cache, &path, settled, counted_hash(&hashes)).unwrap();
        }
        assert_eq!(hashes.get(), 5, "once settled, one hash serves both reads");
        assert_eq!(cache.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_full_cache_starts_over_instead_of_growing() {
        let fixture = Fixture::new();
        let cache = DigestCache::default();
        let stamp = FileStamp::of(&fs::metadata(fixture.executable("seed")).unwrap());
        let seeded = |count: usize| {
            let mut entries = cache.lock().unwrap();
            entries.clear();
            for index in 0..count {
                entries.insert(
                    PathBuf::from(format!("/seed/{index}")),
                    (stamp, String::new()),
                );
            }
        };

        seeded(DIGEST_CACHE_ENTRIES - 1);
        let path = fixture.executable("one");
        cached_digest(&cache, &path, LONG_AFTER, hash_file).unwrap();
        assert_eq!(
            cache.lock().unwrap().len(),
            DIGEST_CACHE_ENTRIES,
            "below the bound nothing is evicted"
        );

        let other = fixture.executable("two");
        cached_digest(&cache, &other, LONG_AFTER, hash_file).unwrap();
        let entries = cache.lock().unwrap();
        assert_eq!(entries.len(), 1, "at the bound the cache is emptied first");
        assert!(entries.contains_key(&other));
    }
}

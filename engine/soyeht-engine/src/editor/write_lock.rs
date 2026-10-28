//! One writer at a time per file, between its comparison and its rename.
//!
//! `write_text_if_unchanged` compares the file with the text it expects, then renames a new file
//! over it. Two jobs for one file (the same folder open in two workspaces gives two editor files)
//! must not both pass the comparison and rename one after the other, or the later rename erases
//! the earlier text without a conflict. The Core lock used to order them; the jobs run off it now.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

static HELD: Mutex<BTreeMap<PathBuf, Arc<Mutex<()>>>> = Mutex::new(BTreeMap::new());

/// Runs `write` while holding the lock of `path` (a canonical path). The lock is dropped from
/// the table when nobody else waits for it.
pub(super) fn exclusive<R>(path: &Path, write: impl FnOnce() -> R) -> R {
    let lock = {
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(held.entry(path.to_owned()).or_default())
    };
    let result = {
        // Nothing inside `write` panics while the guard is held in a way that leaves the
        // protected unit value invalid.
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        write()
    };
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    // The table and this call hold it: nobody else waits.
    if Arc::strong_count(&lock) == 2 {
        held.remove(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{FileSystemPort, ReadOnly, SystemFiles};
    use std::{
        fs,
        sync::{Barrier, atomic::AtomicUsize, atomic::Ordering},
        thread,
    };

    /// Two writers that both read "alpha" and write: exactly one may succeed; the other sees the
    /// first one's text and reports a change instead of overwriting it.
    #[test]
    fn two_writers_of_one_file_never_both_succeed() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-write-lock-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("note.txt");
        for round in 0..20 {
            fs::write(&path, "alpha").unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let wins = Arc::new(AtomicUsize::new(0));
            let writers: Vec<_> = ["one", "two"]
                .into_iter()
                .map(|text| {
                    let (path, barrier, wins) =
                        (path.clone(), Arc::clone(&barrier), Arc::clone(&wins));
                    thread::spawn(move || {
                        barrier.wait();
                        if SystemFiles
                            .write_text_if_unchanged(&path, "alpha", text, ReadOnly::Replace)
                            .unwrap()
                        {
                            wins.fetch_add(1, Ordering::SeqCst);
                        }
                    })
                })
                .collect();
            for writer in writers {
                writer.join().unwrap();
            }
            assert_eq!(wins.load(Ordering::SeqCst), 1, "round {round}");
        }
        // Only this test's own path: other tests write files in the same process.
        assert!(
            !HELD.lock().unwrap().contains_key(&path),
            "the lock is not leaked"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}

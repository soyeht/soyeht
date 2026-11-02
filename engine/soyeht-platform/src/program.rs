//! A program that ends when its own file is gone, on macOS.
//!
//! The registration of a background program names the file it starts. When the person drags the
//! app to the Trash the system stops restarting the program, but one that already runs keeps
//! running from the file's open inode for ever. [`on_gone`] watches the path the process was
//! started from and calls its action once the path has stayed missing for a while, so the
//! program can end itself (ADR 0130).
//!
//! A path that is missing for a moment is not gone: an update that replaces the folder, or a
//! build that writes the file again, must not end a supervisor that holds the person's terminals.
//! The path must be missing at [`MISSES`] checks in a row, [`POLL`] apart. Only a missing path
//! counts; a file that cannot be read for another reason is taken to be there.
//!
//! **Only macOS watches.** On every other system [`on_gone`] has the same shape, returns
//! `Ok(())` and spawns nothing. The Windows installer renames the programs of a running install to
//! `.old` and writes the new ones afterwards (ADR 0123), and the Linux unit skips a start whose
//! program is missing (`ConditionPathExists`, ADR 0121): on those systems the program's path is
//! missing for a while during an upgrade, and ending then would lose the person's terminals with
//! nothing to bring the program back. They can follow when the length of that gap has been
//! measured on each.

#[cfg(any(test, target_os = "macos"))]
mod watching {
    use std::{
        io,
        path::{Path, PathBuf},
        thread,
        time::Duration,
    };

    /// How long to wait between two looks at the file.
    #[cfg(target_os = "macos")]
    pub(super) const POLL: Duration = Duration::from_secs(10);
    /// How many looks in a row must find nothing before the file is gone.
    pub(super) const MISSES: u32 = 3;

    /// Whether anything is at `path`: false only when the system says there is nothing.
    pub(super) fn present(path: &Path) -> bool {
        !matches!(std::fs::metadata(path), Err(error) if error.kind() == io::ErrorKind::NotFound)
    }

    /// Returns after `misses` calls of `present` in a row said no, waiting with `pause` before each.
    pub(super) fn wait_until_gone(
        mut present: impl FnMut() -> bool,
        mut pause: impl FnMut(),
        misses: u32,
    ) {
        let mut missing = 0;
        while missing < misses {
            pause();
            missing = if present() { 0 } else { missing + 1 };
        }
    }

    /// Starts a thread that runs `action` once `path` has been missing `misses` looks in a row.
    pub(crate) fn watch(
        path: PathBuf,
        poll: Duration,
        misses: u32,
        action: impl FnOnce() + Send + 'static,
    ) -> io::Result<()> {
        thread::Builder::new()
            .name("program-file".to_owned())
            .spawn(move || {
                wait_until_gone(|| present(&path), || thread::sleep(poll), misses);
                action();
            })
            .map(drop)
    }
}

#[cfg(test)]
pub(crate) use watching::watch;

/// Starts a thread that runs `action` once, when the file this process was started from is gone.
/// The path is read now, so a later move of the file does not change what is watched. An error
/// means the path could not be read or the thread not started; the program then runs as before.
/// Off macOS this does nothing and returns `Ok(())`, and `action` is dropped without running.
pub fn on_gone(action: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        watching::watch(
            std::env::current_exe()?,
            watching::POLL,
            watching::MISSES,
            action,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        drop(action);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::watching::{MISSES, present, wait_until_gone};
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    /// A file that is looked at once per answer in `answers`, then reads as gone for good.
    fn looks(answers: &[bool]) -> (usize, usize) {
        let looked = Cell::new(0);
        let paused = Cell::new(0);
        wait_until_gone(
            || {
                let answer = answers.get(looked.get()).copied().unwrap_or(false);
                looked.set(looked.get() + 1);
                answer
            },
            || paused.set(paused.get() + 1),
            MISSES,
        );
        (looked.get(), paused.get())
    }

    #[test]
    fn a_file_that_is_there_is_never_gone() {
        let file = std::env::current_exe().unwrap();
        assert!(present(&file));
        assert!(present(file.parent().unwrap()));
    }

    #[test]
    fn a_missing_path_is_gone_and_one_that_cannot_be_read_is_not() {
        let folder = std::env::temp_dir().join(format!("next-program-{}", std::process::id()));
        assert!(!present(&folder.join("soyeht-engine")));
        #[cfg(unix)]
        {
            // A name inside a regular file is not "not found" but "not a directory".
            let file = std::env::current_exe().unwrap();
            assert!(present(&file.join("soyeht-engine")));
        }
    }

    #[test]
    fn the_watch_runs_its_action_when_the_file_is_deleted_and_not_before() {
        let folder =
            std::env::temp_dir().join(format!("next-program-watch-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("soyeht-engine");
        std::fs::write(&file, b"program").unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        watch(file.clone(), Duration::from_millis(20), 3, move || {
            sender.send(()).unwrap();
        })
        .unwrap();
        assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());
        std::fs::remove_file(&file).unwrap();
        receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn three_misses_in_a_row_make_the_file_gone_and_each_look_waits_first() {
        assert_eq!(looks(&[]), (3, 3));
    }

    #[test]
    fn a_file_that_comes_back_starts_the_count_again() {
        // Missing twice, back, then gone: an update that replaced the folder slowly.
        assert_eq!(looks(&[false, false, true]), (6, 6));
    }
}

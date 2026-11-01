//! Helpers for tests that need a file or folder the account cannot reach, or that everybody can,
//! and for measurements that only some systems have.

use crate::process::end_child;
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Held by a test that holds a file lock, and by a test that starts a process. A child that is
/// being started shares every open file until it runs its program, so a lock released while one
/// is starting stays held in the child for a moment, and a test that expects the lock free fails.
#[cfg(test)]
pub(crate) fn serialize_processes() -> std::sync::MutexGuard<'static, ()> {
    static PROCESSES: Mutex<()> = Mutex::new(());
    PROCESSES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A new, empty directory in the system's temporary folder for the test called `name`; it is
/// unique to this process and call. The
/// test removes it when it ends.
///
/// A directory that cannot be made is logged by kind and still returned: the first use of it in
/// the test fails with the real error.
pub fn scratch_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let directory = std::env::temp_dir().join(format!(
        "soyeht-platform-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    // What a killed run with the same process id left is cleared; a directory that is not there
    // is what was wanted.
    if let Err(error) = std::fs::remove_dir_all(&directory)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        crate::log::failure("platform.scratch_dir_failed", &error);
    }
    if let Err(error) = std::fs::create_dir_all(&directory) {
        crate::log::failure("platform.scratch_dir_failed", &error);
    }
    directory
}

/// Takes every permission away from `path`, the way macOS protects a Photos library. Undo it
/// with [`allow_owner`] before removing the folder.
pub fn deny_everyone(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000))
    }
    #[cfg(windows)]
    icacls(path, &["/deny", "*S-1-1-0:(OI)(CI)(F)"])
}

/// Gives `path` back to its owner after [`deny_everyone`]. It does nothing to a path that was
/// never denied.
pub fn allow_owner(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(windows)]
    icacls(path, &["/remove:d", "*S-1-1-0"])
}

/// Lets every local user read the file at `path`.
pub fn expose_to_everyone(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
    }
    #[cfg(windows)]
    icacls(path, &["/grant", "*S-1-1-0:R"])
}

/// The 1, 5 and 15 minute load averages, where the system has them; Windows does not.
pub fn load_average() -> Option<[f64; 3]> {
    #[cfg(unix)]
    {
        let mut averages = [0.0; 3];
        // SAFETY: `averages` remains alive and writable for the call, and the supplied element
        // count exactly matches the three-element buffer.
        let count = unsafe { libc::getloadavg(averages.as_mut_ptr(), averages.len() as i32) };
        (count == averages.len() as i32).then_some(averages)
    }
    #[cfg(windows)]
    None
}

#[cfg(windows)]
fn icacls(path: &Path, arguments: &[&str]) -> io::Result<()> {
    std::process::Command::new("icacls")
        .arg(path)
        .args(arguments)
        .stdout(std::process::Stdio::null())
        .status()?
        .success()
        .then_some(())
        .ok_or_else(|| io::Error::other("icacls failed"))
}

/// The binary called `name` that was built next to `reference` (a `CARGO_BIN_EXE_*` path);
/// Windows adds the `.exe`.
pub fn sibling_binary(reference: &str, name: &str) -> PathBuf {
    PathBuf::from(reference)
        .with_file_name(name)
        .with_extension(std::env::consts::EXE_EXTENSION)
}

/// The longest [`warm_up`] waits for a program that does not end by itself.
const WARM_UP_DEADLINE: Duration = Duration::from_secs(120);

/// Runs `binary` once with an argument it does not know, the first time this process asks, and
/// returns how long that took (zero when it already ran).
///
/// The first run of a program the linker just wrote is slow where the system assesses new
/// executables before it lets them run: about a second for the engine, and seconds more when
/// many test processes start new programs at once, as a merge gate does on a loaded Mac. Later
/// runs take milliseconds. A test that starts a program and waits for it to be ready under a
/// deadline calls this first, so the deadline measures the program and not the assessment.
/// Every program of this repository exits at once on an unknown argument.
pub fn warm_up(binary: &Path) -> Duration {
    static WARMED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
    // Held while the program runs: a second caller for the same program waits for the first
    // instead of starting another assessment.
    let mut warmed = WARMED.lock().unwrap_or_else(PoisonError::into_inner);
    let started = Instant::now();
    if !warmed.insert(binary.to_owned()) {
        return Duration::ZERO;
    }
    // A program that cannot start is reported by the test that starts it for real.
    if let Ok(mut child) = Command::new(binary)
        .arg("--warm-up")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        while matches!(child.try_wait(), Ok(None)) {
            if started.elapsed() > WARM_UP_DEADLINE {
                // It exits or is already gone; either way nothing is left to wait for.
                end_child(&mut child);
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
    started.elapsed()
}

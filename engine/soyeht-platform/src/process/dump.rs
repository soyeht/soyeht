//! Keeping this process's memory from other programs of the same user.

use std::io;

/// Marks this process as one that cannot be dumped or attached to by another program of the same
/// user (`prctl(PR_SET_DUMPABLE, 0)` on Linux), so with the usual `ptrace_scope` it cannot be
/// debugged or read through `/proc/<pid>/mem`. An exec clears the mark, so a process calls this
/// once it runs its final program and not in a launcher. It does not stop the superuser.
///
/// # Errors
/// [`io::ErrorKind::Unsupported`] on a system with no such mark, and the system's error otherwise.
pub fn non_dumpable() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: PR_SET_DUMPABLE takes plain integers and touches no memory.
        let set = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
        if set == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn the_process_reads_as_non_dumpable_afterwards() {
        non_dumpable().unwrap();
        // SAFETY: PR_GET_DUMPABLE takes plain integers and touches no memory.
        let state = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
        assert_eq!(state, 0);
    }
}

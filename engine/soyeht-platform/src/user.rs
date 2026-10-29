//! Who is running this process.

use std::{io, path::PathBuf};

/// A stable text identity of the user running this process: the numeric user id on Unix and
/// the security identifier (SID) on Windows.
pub fn id() -> io::Result<String> {
    system::id()
}

/// The shell a person's terminals and shell probes start, chosen by one rule on every caller.
///
/// On Unix: `$SHELL` when it names an existing absolute file (the login sets it; a program a
/// desktop session or a service started may carry none, or a stale one), else the shell the
/// account's entry in the user database names, unless that shell exists to refuse a login
/// (`nologin`, `false`) or is not an existing absolute file, else the first `bash` there is,
/// else the POSIX `sh`. The user database is asked only when `$SHELL` is not usable, and its
/// answer is remembered for the life of the process, so a caller on a hot path or under a lock
/// asks it once at start (the lookup may reach a directory service) and never again.
///
/// Windows has no login shell: it returns an `Unsupported` error.
///
/// # Errors
/// Only where the system has no such shell, which is Windows.
pub fn preferred_shell() -> io::Result<PathBuf> {
    system::preferred_shell()
}

#[cfg(unix)]
mod system {
    use std::{
        ffi::{CStr, OsStr, OsString},
        io,
        os::unix::ffi::OsStrExt as _,
        path::{Path, PathBuf},
        ptr,
        sync::OnceLock,
    };

    /// The largest buffer `getpwuid_r` is offered: a record is a few hundred bytes.
    const MAX_RECORD: usize = 1 << 20;

    pub(super) fn preferred_shell() -> io::Result<PathBuf> {
        Ok(choose_shell(std::env::var_os("SHELL"), account_shell))
    }

    /// The shell the user database names for this account, read once per process. A failed
    /// lookup is logged by kind and names none, which is correct here: the fallbacks after it
    /// start a shell all the same.
    fn account_shell() -> Option<PathBuf> {
        static ACCOUNT: OnceLock<Option<PathBuf>> = OnceLock::new();
        ACCOUNT
            .get_or_init(|| match login_shell() {
                Ok(shell) => shell,
                Err(error) => {
                    crate::log::failure("platform.login_shell_lookup_failed", &error);
                    None
                }
            })
            .clone()
    }

    /// An account shell that exists to refuse a login: starting it ends the terminal at once.
    fn refuses_login(shell: &Path) -> bool {
        matches!(
            shell.file_name().and_then(|name| name.to_str()),
            Some("nologin" | "false")
        )
    }

    /// `environment`, else `account()` (asked only when `environment` is not usable), each only
    /// when it names an existing absolute file, else the first `bash` there is, else `sh`.
    fn choose_shell(
        environment: Option<OsString>,
        account: impl FnOnce() -> Option<PathBuf>,
    ) -> PathBuf {
        let usable = |shell: PathBuf| (shell.is_absolute() && shell.is_file()).then_some(shell);
        environment
            .map(PathBuf::from)
            .and_then(usable)
            .or_else(|| {
                account()
                    .filter(|shell| !refuses_login(shell))
                    .and_then(usable)
            })
            .unwrap_or_else(|| {
                ["/bin/bash", "/usr/bin/bash"]
                    .into_iter()
                    .map(PathBuf::from)
                    .find(|candidate| candidate.is_file())
                    .unwrap_or_else(|| PathBuf::from("/bin/sh"))
            })
    }

    fn login_shell() -> io::Result<Option<PathBuf>> {
        // SAFETY: sysconf only reads a limit.
        let hint = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
        // -1 (no limit known) or 0 asks for the usual size.
        let mut size = match usize::try_from(hint) {
            Ok(size) if size > 0 => size,
            _ => 1024,
        };
        // SAFETY: geteuid has no arguments or memory preconditions.
        let user = unsafe { libc::geteuid() };
        loop {
            let mut buffer = vec![0_u8; size];
            // SAFETY: an all-zero passwd is a valid value to be overwritten (null pointers).
            let mut record: libc::passwd = unsafe { std::mem::zeroed() };
            let mut found: *mut libc::passwd = ptr::null_mut();
            // SAFETY: `record` and `buffer` outlive the call, `buffer.len()` is the size given,
            // and `found` is an out pointer.
            let status = unsafe {
                libc::getpwuid_r(
                    user,
                    &mut record,
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    &mut found,
                )
            };
            match status {
                0 if found.is_null() => return Ok(None),
                0 => {
                    if record.pw_shell.is_null() {
                        return Ok(None);
                    }
                    // SAFETY: on success `pw_shell` is a NUL-terminated string inside `buffer`,
                    // which is alive here; it is copied before the buffer is dropped.
                    let shell = unsafe { CStr::from_ptr(record.pw_shell) }.to_bytes();
                    return Ok((!shell.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(shell))));
                }
                libc::ERANGE if size < MAX_RECORD => size *= 2,
                code => return Err(io::Error::from_raw_os_error(code)),
            }
        }
    }

    pub(super) fn id() -> io::Result<String> {
        // SAFETY: geteuid has no arguments or memory preconditions.
        Ok(unsafe { libc::geteuid() }.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;

        /// A scratch directory removed when dropped.
        struct TempDir(PathBuf);

        impl TempDir {
            fn new(name: &str) -> Self {
                static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let path = std::env::temp_dir().join(format!(
                    "soyeht-platform-user-{name}-{}-{}",
                    std::process::id(),
                    COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                fs::create_dir_all(&path).unwrap();
                Self(path)
            }

            fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                drop(fs::remove_dir_all(&self.0));
            }
        }

        fn file_named(directory: &TempDir, name: &str) -> PathBuf {
            let path = directory.path().join(name);
            fs::write(&path, "#!/bin/sh\n").unwrap();
            path
        }

        fn must_not_be_asked() -> Option<PathBuf> {
            panic!("the lookup reads a directory service")
        }

        fn is_a_fallback(shell: &Path) -> bool {
            shell.file_name().is_some_and(|name| name == "bash") || shell == Path::new("/bin/sh")
        }

        #[test]
        fn the_account_login_shell_is_read_from_the_user_database_not_the_environment() {
            // A CI container may run as a user with no entry or no shell; an entry names a program.
            if let Some(shell) = login_shell().unwrap() {
                assert!(shell.is_absolute(), "{shell:?}");
            }
        }

        #[test]
        fn the_shell_variable_beats_the_account_and_the_account_is_not_asked() {
            let directory = TempDir::new("shell-variable");
            let mine = file_named(&directory, "fish");
            assert_eq!(
                choose_shell(Some(mine.clone().into_os_string()), must_not_be_asked),
                mine
            );
        }

        #[test]
        fn a_stale_shell_variable_falls_back_to_the_account_shell() {
            let directory = TempDir::new("stale-variable");
            let account = file_named(&directory, "zsh");
            for stale in [
                None,
                Some(OsString::new()),
                Some("zsh".into()),
                Some("/nowhere/at/all/zsh".into()),
                // A directory is not a shell.
                Some(directory.path().as_os_str().to_owned()),
            ] {
                assert_eq!(
                    choose_shell(stale.clone(), || Some(account.clone())),
                    account,
                    "{stale:?}"
                );
            }
        }

        #[test]
        fn an_account_shell_that_refuses_logins_is_not_started() {
            assert!(refuses_login(Path::new("/usr/sbin/nologin")));
            assert!(refuses_login(Path::new("/sbin/nologin")));
            assert!(refuses_login(Path::new("/bin/false")));
            assert!(!refuses_login(Path::new("/bin/bash")));
            // The same through the choice, with files that exist under those names.
            let directory = TempDir::new("refusing");
            for name in ["nologin", "false"] {
                let refusing = file_named(&directory, name);
                let chosen = choose_shell(None, || Some(refusing.clone()));
                assert!(is_a_fallback(&chosen), "{name}: {chosen:?}");
            }
        }

        #[test]
        fn no_usable_shell_falls_back_to_bash_then_to_sh() {
            let directory = TempDir::new("fallbacks");
            let missing = directory.path().join("missing");
            for account in [None, Some(missing)] {
                let chosen = choose_shell(None, || account.clone());
                assert!(is_a_fallback(&chosen), "{chosen:?}");
                assert!(chosen.is_file(), "{chosen:?}");
            }
            let bash_exists = ["/bin/bash", "/usr/bin/bash"]
                .into_iter()
                .any(|path| Path::new(path).is_file());
            if !bash_exists {
                assert_eq!(choose_shell(None, || None), Path::new("/bin/sh"));
            }
        }

        #[test]
        fn the_account_is_looked_up_once_per_process() {
            // The same value is returned from the cell on every call; a changed user database is
            // not read again.
            assert_eq!(account_shell(), account_shell());
        }

        #[test]
        fn the_preferred_shell_exists_on_a_unix_host() {
            assert!(preferred_shell().unwrap().is_file());
        }
    }
}

#[cfg(windows)]
mod system {
    use std::{io, path::PathBuf, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    pub(super) fn preferred_shell() -> io::Result<PathBuf> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub(super) fn id() -> io::Result<String> {
        let mut token: HANDLE = ptr::null_mut();
        // SAFETY: the pseudo-handle of the current process is always valid and `token` is an
        // out pointer that lives for the call.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = sid_of(token);
        // SAFETY: `token` was opened above and is closed once.
        unsafe { CloseHandle(token) };
        result
    }

    fn sid_of(token: HANDLE) -> io::Result<String> {
        let mut size = 0;
        // SAFETY: a null buffer with size 0 only asks for the size needed.
        unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size) };
        let mut buffer = vec![0_u64; (size as usize).div_ceil(8).max(1)];
        // SAFETY: `buffer` holds at least `size` bytes, 8-byte aligned, and the call fills it.
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a successful TokenUser query wrote a TOKEN_USER at the start of the buffer.
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut text = ptr::null_mut();
        // SAFETY: the SID belongs to the buffer, which outlives the call; `text` is an out
        // pointer.
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call returned a NUL-terminated wide string that this code frees once.
        let sid = unsafe {
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
            sid
        };
        Ok(sid)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_user_has_an_identity() {
        assert!(!super::id().unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn windows_has_no_preferred_shell() {
        assert_eq!(
            super::preferred_shell().unwrap_err().kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}

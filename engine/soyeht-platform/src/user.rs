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
/// else the POSIX `sh`. A path that is not UTF-8 is never chosen, so the result can always be an
/// argument of a command line. The user database is asked only when `$SHELL` is not usable, and its
/// answer is remembered for the life of the process, so a caller on a hot path or under a lock
/// asks it once at start (the lookup may reach a directory service) and never again.
///
/// Windows has no login shell: it returns an `Unsupported` error.
///
/// # Errors
/// `Unsupported` on Windows; a `NotFound` error on a Unix host that has not even a `sh`.
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
        choose_shell(std::env::var_os("SHELL"), account_shell, LAST_RESORTS)
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    /// The shells tried, in this order, when neither `$SHELL` nor the account names a usable one.
    const LAST_RESORTS: &[&str] = &["/bin/bash", "/usr/bin/bash", "/bin/sh"];

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
    /// when it names an existing absolute file with a UTF-8 path, else the first of `last_resorts`
    /// that is a file; `None` when there is none.
    fn choose_shell(
        environment: Option<OsString>,
        account: impl FnOnce() -> Option<PathBuf>,
        last_resorts: &[&str],
    ) -> Option<PathBuf> {
        let usable = |shell: PathBuf| {
            (shell.is_absolute() && shell.to_str().is_some() && shell.is_file()).then_some(shell)
        };
        environment
            .map(PathBuf::from)
            .and_then(usable)
            .or_else(|| {
                account()
                    .filter(|shell| !refuses_login(shell))
                    .and_then(usable)
            })
            .or_else(|| {
                last_resorts
                    .iter()
                    .map(PathBuf::from)
                    .find(|candidate| candidate.is_file())
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
        use crate::testing::scratch_dir;
        use std::{ffi::OsStr, fs};

        /// A file named `name` in `directory`, standing for a shell.
        fn shell_named(directory: &Path, name: &str) -> PathBuf {
            let path = directory.join(name);
            fs::write(&path, "#!/bin/sh\n").unwrap();
            path
        }

        fn text(path: &Path) -> &str {
            path.to_str().unwrap()
        }

        fn must_not_be_asked() -> Option<PathBuf> {
            panic!("the lookup reads a directory service")
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
            let directory = scratch_dir("shell-variable");
            let mine = shell_named(&directory, "fish");
            assert_eq!(
                choose_shell(Some(mine.clone().into()), must_not_be_asked, &[]),
                Some(mine)
            );
            fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn a_stale_shell_variable_falls_back_to_the_account_shell() {
            let directory = scratch_dir("stale-variable");
            let account = shell_named(&directory, "zsh");
            for stale in [
                None,
                Some(OsString::new()),
                Some("zsh".into()),
                Some("/nowhere/at/all/zsh".into()),
                // A directory is not a shell.
                Some(directory.as_os_str().to_owned()),
            ] {
                assert_eq!(
                    choose_shell(stale.clone(), || Some(account.clone()), &[]),
                    Some(account.clone()),
                    "{stale:?}"
                );
            }
            fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn a_shell_whose_path_is_not_text_is_never_chosen() {
            let directory = scratch_dir("not-utf8");
            let name = OsStr::from_bytes(b"sh-\xff\xfe");
            let odd = directory.join(name);
            // Some file systems (APFS) refuse a name that is not UTF-8; there is nothing to test.
            if fs::write(&odd, "#!/bin/sh\n").is_err() {
                fs::remove_dir_all(directory).unwrap();
                return;
            }
            assert!(odd.is_file() && odd.to_str().is_none());
            let account = shell_named(&directory, "zsh");
            // As `$SHELL`, it is passed over for the account's shell.
            assert_eq!(
                choose_shell(Some(odd.clone().into()), || Some(account.clone()), &[]),
                Some(account.clone())
            );
            // As the account's shell, for the last resorts.
            let resort = shell_named(&directory, "bash");
            assert_eq!(
                choose_shell(None, || Some(odd.clone()), &[text(&resort)]),
                Some(resort)
            );
            fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn an_account_shell_that_refuses_logins_is_not_started() {
            assert!(refuses_login(Path::new("/usr/sbin/nologin")));
            assert!(refuses_login(Path::new("/sbin/nologin")));
            assert!(refuses_login(Path::new("/bin/false")));
            assert!(!refuses_login(Path::new("/bin/bash")));
            // The same through the choice, with files that exist under those names.
            let directory = scratch_dir("refusing");
            let resort = shell_named(&directory, "bash");
            for name in ["nologin", "false"] {
                let refusing = shell_named(&directory, name);
                assert_eq!(
                    choose_shell(None, || Some(refusing.clone()), &[text(&resort)]),
                    Some(resort.clone()),
                    "{name}"
                );
            }
            fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn no_usable_shell_falls_back_to_the_first_last_resort_that_exists() {
            let directory = scratch_dir("fallbacks");
            let missing = directory.join("missing");
            let bash = shell_named(&directory, "bash");
            let sh = shell_named(&directory, "sh");
            let order = [text(&missing), text(&bash), text(&sh)];
            for account in [None, Some(missing.clone())] {
                let chosen = choose_shell(None, || account.clone(), &order);
                assert_eq!(chosen, Some(bash.clone()));
            }
            // Bash gone: the POSIX shell. Nothing at all: no shell, not a made-up one.
            assert_eq!(
                choose_shell(None, || None, &[text(&missing), text(&sh)]),
                Some(sh)
            );
            assert_eq!(choose_shell(None, || None, &[text(&missing)]), None);
            fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn the_account_is_looked_up_once_per_process() {
            // The same value is returned from the cell on every call; a changed user database is
            // not read again.
            assert_eq!(account_shell(), account_shell());
        }

        #[test]
        fn the_preferred_shell_exists_and_is_text_on_a_unix_host() {
            let shell = preferred_shell().unwrap();
            assert!(shell.is_file() && shell.to_str().is_some(), "{shell:?}");
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

//! Who is running this process.

use std::{io, path::PathBuf};

/// A stable text identity of the user running this process: the numeric user id on Unix and
/// the security identifier (SID) on Windows.
pub fn id() -> io::Result<String> {
    system::id()
}

/// The login shell of the account running this process, as the system's user database names it,
/// or `None` when the account has none. It does not depend on the environment, which a program
/// started from a desktop session or a service may not carry (`SHELL` is set by the login, not
/// by the system). Windows has no login shell: it returns an `Unsupported` error.
///
/// The lookup may read a directory service, so it is not for a hot path.
pub fn login_shell() -> io::Result<Option<PathBuf>> {
    system::login_shell()
}

#[cfg(unix)]
mod system {
    use std::{
        ffi::{CStr, OsStr},
        io,
        os::unix::ffi::OsStrExt as _,
        path::PathBuf,
        ptr,
    };

    /// The largest buffer `getpwuid_r` is offered: a record is a few hundred bytes.
    const MAX_RECORD: usize = 1 << 20;

    pub(super) fn login_shell() -> io::Result<Option<PathBuf>> {
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

    pub(super) fn login_shell() -> io::Result<Option<PathBuf>> {
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

    #[cfg(unix)]
    #[test]
    fn the_account_login_shell_is_read_from_the_user_database_not_the_environment() {
        // A CI container may run as a user with no entry or no shell; an entry names a program.
        if let Some(shell) = super::login_shell().unwrap() {
            assert!(shell.is_absolute(), "{shell:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_has_no_login_shell() {
        assert_eq!(
            super::login_shell().unwrap_err().kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}

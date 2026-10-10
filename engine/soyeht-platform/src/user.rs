//! Who is running this process.

use std::io;

/// A stable text identity of the user running this process: the numeric user id on Unix and
/// the security identifier (SID) on Windows.
pub fn id() -> io::Result<String> {
    system::id()
}

#[cfg(unix)]
mod system {
    use std::io;

    pub(super) fn id() -> io::Result<String> {
        // SAFETY: geteuid has no arguments or memory preconditions.
        Ok(unsafe { libc::geteuid() }.to_string())
    }
}

#[cfg(windows)]
mod system {
    use std::{io, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

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
}

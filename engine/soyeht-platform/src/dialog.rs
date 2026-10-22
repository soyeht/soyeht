//! A native message for a program that has no terminal.
//!
//! A release desktop client on Windows is a GUI-subsystem program: it has no console, so a
//! failure before its window exists (no usable graphics driver, no profile directory) would be
//! seen by nobody. [`error`] shows that failure in a message box. Elsewhere it does nothing:
//! a program on Linux or macOS that is started from a terminal prints the text to it, and the
//! log file keeps it either way.

/// Shows `text` under `title` in a modal message box and returns when it is closed. The caller
/// logs the text as well; this is only for the person who has no log to read.
pub fn error(title: &str, text: &str) {
    system::error(title, text);
}

#[cfg(windows)]
mod system {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    pub(super) fn error(title: &str, text: &str) {
        let wide =
            |text: &str| -> Vec<u16> { OsStr::new(text).encode_wide().chain(Some(0)).collect() };
        let (title, text) = (wide(title), wide(text));
        // SAFETY: both buffers are NUL-terminated UTF-16 that outlive the call, and a null
        // owner window is allowed. The result (which button closed it) carries nothing.
        unsafe {
            MessageBoxW(
                ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

#[cfg(not(windows))]
mod system {
    pub(super) fn error(_title: &str, _text: &str) {}
}

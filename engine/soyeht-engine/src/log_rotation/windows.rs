//! Standard error as the process's standard error handle.

use std::{
    fs::File,
    io::{self, PipeWriter},
    os::windows::{
        ffi::OsStringExt,
        io::{AsHandle, AsRawHandle, OwnedHandle},
    },
    path::PathBuf,
};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW},
    System::Console::{STD_ERROR_HANDLE, SetStdHandle},
};

/// A saved copy of what standard error was.
pub(super) type Original = OwnedHandle;

pub(super) fn current_stderr() -> io::Result<Original> {
    io::stderr().as_handle().try_clone_to_owned()
}

/// Makes `writer` the process's standard error. The caller keeps `writer` open while it is.
pub(super) fn redirect_stderr(writer: &PipeWriter) -> io::Result<()> {
    set_stderr(writer.as_raw_handle())
}

/// Puts back what standard error was.
pub(super) fn restore_stderr(original: &Original) -> io::Result<()> {
    set_stderr(original.as_raw_handle())
}

fn set_stderr(handle: HANDLE) -> io::Result<()> {
    // SAFETY: the handle is open and the caller keeps it open while it is standard error.
    if unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The first guess at a path's length in UTF-16 units; the call reports a longer one.
const PATH_UNITS: u32 = 1024;

pub(super) fn descriptor_path(file: &File) -> Option<PathBuf> {
    let mut buffer = vec![0_u16; PATH_UNITS as usize];
    loop {
        // SAFETY: the handle is open and `buffer` is as long as the length passed.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                // The buffer only grows to a length the system reported as a u32.
                u32::try_from(buffer.len()).unwrap_or(u32::MAX),
                FILE_NAME_NORMALIZED,
            )
        } as usize;
        match length {
            0 => return None,
            length if length > buffer.len() => buffer.resize(length, 0),
            length => {
                let path = std::ffi::OsString::from_wide(&buffer[..length]);
                // The system returns the extended form `\\?\C:\...`.
                let path = path.to_string_lossy();
                return Some(PathBuf::from(path.strip_prefix(r"\\?\").unwrap_or(&path)));
            }
        }
    }
}

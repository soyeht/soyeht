use std::{
    fs::{self, File, OpenOptions},
    io,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_LOCK_VIOLATION, ERROR_SUCCESS, HANDLE, LocalFree},
    Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_FILE_OBJECT,
        },
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        SetFileSecurityW,
    },
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, LOCKFILE_EXCLUSIVE_LOCK,
        LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, MoveFileExW, READ_CONTROL,
    },
    System::{
        IO::{DeviceIoControl, OVERLAPPED},
        Ioctl::FSCTL_SET_REPARSE_POINT,
        SystemServices::IO_REPARSE_TAG_MOUNT_POINT,
    },
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub(super) fn set_private(path: &Path, directory: bool) -> io::Result<()> {
    let sid = crate::user::id()?;
    // A protected DACL (no inherited entries) that gives the user and the system full access;
    // for a directory the entries also pass to the files and folders created inside it.
    let flags = if directory { "OICI" } else { "" };
    set_access_list(
        path,
        &format!("D:P(A;{flags};FA;;;{sid})(A;{flags};FA;;;SY)"),
    )
}

/// Makes the directory at `path` owner-only, by path: Windows has no open that refuses a link
/// and changes the access list of what it opened in one step, so the parent of `path` must be a
/// directory only the owner can change (callers check the directory just before, and an attacker
/// who can swap entries of the parent could redirect the change).
pub(super) fn set_private_dir_no_follow(path: &Path) -> io::Result<()> {
    set_private(path, true)
}

/// Windows' version of [`set_private_dir_no_follow`] has no such refusal: the caller checked.
pub(super) fn is_link_or_not_directory(_error: &io::Error) -> bool {
    false
}

/// Replaces the access list of `path` with the one written in SDDL text.
pub(super) fn set_access_list(path: &Path, sddl: &str) -> io::Result<()> {
    let sddl = wide(sddl);
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated and `descriptor` is an out pointer.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let result = apply(path, descriptor);
    // SAFETY: the descriptor was allocated by the conversion call and is freed once.
    unsafe { LocalFree(descriptor) };
    result
}

/// Sets the descriptor's access list on `path`. `SetFileSecurityW` is used rather than
/// `SetNamedSecurityInfoW` because the latter also rewrites every file below a folder, which
/// would stop at the first file another process holds open.
fn apply(path: &Path, descriptor: PSECURITY_DESCRIPTOR) -> io::Result<()> {
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `name` is NUL-terminated and `descriptor` is a valid descriptor that carries the
    // protected flag of the access list.
    let changed = unsafe {
        SetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    if changed == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Drops the `\\?\` of a drive path; a network path (`\\?\UNC\...`) keeps its form.
pub(super) fn readable(canonical: std::path::PathBuf) -> std::path::PathBuf {
    match canonical.to_str() {
        Some(text) if !text.starts_with(r"\\?\UNC\") => text
            .strip_prefix(r"\\?\")
            .map_or(canonical.clone(), std::path::PathBuf::from),
        _ => canonical,
    }
}

pub(super) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    let resolved = match link.parent() {
        Some(parent) if !target.is_absolute() => parent.join(target),
        _ => target.to_owned(),
    };
    if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

pub(super) fn replace_dir_link(path: &Path, target: &Path) -> io::Result<()> {
    let resolved = match (target.is_absolute(), path.parent()) {
        (false, Some(parent)) => parent.join(target),
        _ => target.to_owned(),
    };
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if let (Ok(current), Ok(wanted)) = (
                crate::fs::canonicalize(path),
                crate::fs::canonicalize(&resolved),
            ) && current == wanted
            {
                return Ok(());
            }
            // Both a directory symbolic link and a junction are removed as an empty directory.
            fs::remove_dir(path)?;
        }
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a folder is where the link belongs",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if std::os::windows::fs::symlink_dir(&resolved, path).is_ok() {
        return Ok(());
    }
    fs::create_dir_all(&resolved)?;
    junction(path, &resolved)
}

/// A junction needs no privilege. It is a folder with a mount-point reparse record naming the
/// target, which the system writes when asked to; no command interpreter is involved, so no
/// character of either name is read as anything but part of the name.
pub(super) fn junction(path: &Path, target: &Path) -> io::Result<()> {
    let record = junction_record(&crate::fs::canonicalize(target)?)?;
    fs::create_dir(path)?;
    let attached = attach_record(path, &record);
    if attached.is_err() {
        // Leave nothing behind that looks like a link but is an empty folder.
        drop(fs::remove_dir(path));
    }
    attached
}

fn attach_record(path: &Path, record: &[u8]) -> io::Result<()> {
    let folder = OpenOptions::new()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let mut returned = 0;
    // SAFETY: the handle is an open folder, `record` is a complete reparse record of the length
    // passed, and the call has no output buffer.
    let set = unsafe {
        DeviceIoControl(
            folder.as_raw_handle() as HANDLE,
            FSCTL_SET_REPARSE_POINT,
            record.as_ptr().cast(),
            u32::try_from(record.len()).map_err(|_| io::Error::other("name too long"))?,
            ptr::null_mut(),
            0,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if set == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The mount-point reparse record (`REPARSE_DATA_BUFFER`) that makes a folder a junction to the
/// absolute drive path `target`.
fn junction_record(target: &Path) -> io::Result<Vec<u8>> {
    let drive_path = target.as_os_str().to_string_lossy();
    if drive_path.starts_with(r"\\") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a junction cannot point at a network path",
        ));
    }
    let substitute: Vec<u16> = format!(r"\??\{drive_path}").encode_utf16().collect();
    let print: Vec<u16> = drive_path.encode_utf16().collect();
    fn to_bytes(units: &[u16]) -> impl Iterator<Item = u8> + '_ {
        units.iter().flat_map(|unit| unit.to_le_bytes())
    }
    // Both names are followed by a NUL that the lengths do not count.
    let mut names: Vec<u8> = to_bytes(&substitute).collect();
    names.extend([0, 0]);
    let print_offset = names.len();
    names.extend(to_bytes(&print));
    names.extend([0, 0]);
    let word = |value: usize| u16::try_from(value).map_err(|_| io::Error::other("name too long"));
    let mut record = Vec::new();
    record.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    record.extend(word(8 + names.len())?.to_le_bytes());
    record.extend([0, 0]);
    record.extend(0_u16.to_le_bytes());
    record.extend(word(substitute.len() * 2)?.to_le_bytes());
    record.extend(word(print_offset)?.to_le_bytes());
    record.extend(word(print.len() * 2)?.to_le_bytes());
    record.extend(names);
    Ok(record)
}

pub(super) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both names are NUL-terminated; flags 0 mean the move fails if `to` exists.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(super) fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir(path)?;
    set_private(path, true)
}

/// Windows judges the owner with the access list, which names the user (`dir_is_owner_only`).
pub(super) fn is_owned_by_current_user(_: &fs::Metadata) -> bool {
    true
}

pub(super) fn dir_is_owner_only(path: &Path) -> io::Result<bool> {
    // Reading the access list needs only READ_CONTROL; backup semantics let a folder be opened,
    // and the reparse flag judges a link as itself.
    let handle = OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    file_is_owner_only(&handle)
}

pub(super) fn file_is_owner_only(file: &File) -> io::Result<bool> {
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle is open for the call and `descriptor` is an out pointer.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    judge(status, descriptor)
}

/// Judges the access list of `descriptor`, which the query allocated, and frees it.
fn judge(status: u32, descriptor: PSECURITY_DESCRIPTOR) -> io::Result<bool> {
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: the descriptor is valid until freed below and `text` is an out pointer.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            ptr::null_mut(),
        )
    };
    let sddl = if converted == 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: the conversion returned a NUL-terminated wide string that is freed once.
        Ok(unsafe {
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let sddl = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
            sddl
        })
    };
    // SAFETY: the descriptor was allocated by the query above and is freed once.
    unsafe { LocalFree(descriptor) };
    Ok(super::sddl::allows_only_owner(&sddl?, &crate::user::id()?))
}

/// A directory cannot be flushed on Windows; the file system journals the rename.
pub(super) fn sync_directory(_: &Path) -> io::Result<()> {
    Ok(())
}

/// The file takes the ACL of its directory; there is no per-open mode on Windows.
pub(super) fn owner_only(options: &mut OpenOptions) -> &mut OpenOptions {
    options
}

pub(super) fn open_no_follow(options: &OpenOptions, path: &Path) -> io::Result<File> {
    let file = options
        .clone()
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    // With the reparse-point flag a link opens as itself, so a link is visible here.
    if file.metadata()?.file_type().is_symlink() {
        return Err(io::Error::other("refusing to follow a link"));
    }
    Ok(file)
}

/// Takes the exclusive lock of `file`; `false` when another process holds it.
///
/// Windows locks are mandatory: a locked range cannot be read by anyone else. The lock is on one
/// byte far past the end of the file, which keeps the file itself readable (a project search
/// walks every file under a folder, this one included) and still excludes a second holder.
pub(super) fn try_lock(file: &File) -> io::Result<bool> {
    // SAFETY: OVERLAPPED is plain data; zero means no event and offset 0 until set below.
    let mut position: OVERLAPPED = unsafe { std::mem::zeroed() };
    position.Anonymous.Anonymous.OffsetHigh = 0x7fff_0000;
    // SAFETY: the handle is open for the duration of the call and `position` outlives it.
    let locked = unsafe {
        LockFileEx(
            file.as_raw_handle() as HANDLE,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut position,
        )
    };
    if locked != 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error().map(i32::cast_unsigned) == Some(ERROR_LOCK_VIOLATION) {
        Ok(false)
    } else {
        Err(error)
    }
}

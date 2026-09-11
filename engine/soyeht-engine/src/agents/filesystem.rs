//! Descriptor-relative text access. No component of an agent-supplied path
//! follows a symlink. Atomic replacement never truncates another hard link.
use soyeht_protocol::limits;
use std::{
    ffi::CString,
    fs::File,
    io::{self, Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    path::{Component, Path},
};
use uuid::Uuid;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
fn name(text: &std::ffi::OsStr) -> io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(text.as_bytes()).map_err(|_| invalid("invalid path component"))
}
fn child(parent: &File, name: &CString, flags: i32, mode: libc::mode_t) -> io::Result<File> {
    // SAFETY: parent owns a live directory descriptor and name is terminated.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn parent(root: &Path, path: &Path) -> io::Result<(File, CString)> {
    if !root.is_absolute() || !path.is_absolute() {
        return Err(invalid("absolute paths are required"));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid("path is outside the worktree"))?;
    let mut parts = relative.components().collect::<Vec<_>>();
    let Some(Component::Normal(leaf)) = parts.pop() else {
        return Err(invalid("a file inside the worktree is required"));
    };
    if parts
        .iter()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid("parent traversal is not allowed"));
    }
    let mut directory = File::open("/")?;
    for part in root.components().chain(parts) {
        match part {
            Component::RootDir => (),
            Component::Normal(part) => {
                directory = child(
                    &directory,
                    &name(part)?,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?
            }
            _ => return Err(invalid("worktree path must be canonical")),
        }
    }
    Ok((directory, name(leaf)?))
}

pub fn read(root: &Path, path: &Path) -> io::Result<String> {
    let (directory, leaf) = parent(root, path)?;
    let file = child(&directory, &leaf, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("text access requires a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limits::ACP_MESSAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limits::ACP_MESSAGE_BYTES {
        return Err(io::Error::other("text file exceeds 1 MiB"));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::other("text file is not UTF-8"))
}

pub fn permits_edit(root: &Path, path: &Path) -> bool {
    let Ok((directory, leaf)) = parent(root, path) else {
        return false;
    };
    match child(&directory, &leaf, libc::O_RDONLY | libc::O_NONBLOCK, 0) {
        Ok(file) => file.metadata().is_ok_and(|m| m.is_file()),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

pub fn write(root: &Path, path: &Path, content: &str) -> io::Result<()> {
    write_with_key(root, path, content, &Uuid::new_v4().to_string())
}

pub fn write_with_key(root: &Path, path: &Path, content: &str, operation: &str) -> io::Result<()> {
    let operation =
        Uuid::parse_str(operation).map_err(|_| invalid("invalid write operation identity"))?;
    if content.len() > limits::ACP_MESSAGE_BYTES {
        return Err(io::Error::other("text content exceeds 1 MiB"));
    }
    let (directory, leaf) = parent(root, path)?;
    let mode = match child(&directory, &leaf, libc::O_RDONLY | libc::O_NONBLOCK, 0) {
        Ok(file) => {
            use std::os::unix::fs::PermissionsExt;
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                return Err(invalid("text access requires a regular file"));
            }
            metadata.permissions().mode() & 0o777
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0o600,
        Err(error) => return Err(error),
    };
    let temporary = CString::new(format!(".soyeht-write-{operation}"))
        .map_err(|_| invalid("invalid temporary file name"))?;
    let mut file = child(
        &directory,
        &temporary,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    let result = (|| {
        file.write_all(content.as_bytes())?;
        // SAFETY: file owns a live descriptor; only ordinary permission bits survive.
        if unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        file.sync_all()?;
        // SAFETY: both names are terminated and both directory descriptors live.
        if unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                leaf.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        directory.sync_all()
    })();
    if result.is_err() {
        // SAFETY: cleanup names only this invocation's exclusive temporary file.
        let _ = unsafe { libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::PathBuf,
    };
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn outside_parent_symlink_and_final_symlink_are_rejected_without_changing_the_target() {
        let fixture = Directory(std::env::temp_dir().join(format!("sn-files-{}", Uuid::new_v4())));
        fs::create_dir_all(fixture.0.join("work")).unwrap();
        let base = fixture.0.canonicalize().unwrap();
        let root = base.join("work");
        let outside = base.join("outside");
        fs::write(&outside, "private").unwrap();
        symlink(&base, root.join("link")).unwrap();
        symlink(&outside, root.join("file-link")).unwrap();
        for path in [
            &outside,
            &root.join("../outside"),
            &root.join("link/outside"),
            &root.join("file-link"),
        ] {
            assert!(read(&root, path).is_err());
            assert!(write(&root, path, "changed").is_err());
        }
        assert_eq!(fs::read_to_string(&outside).unwrap(), "private");
        let file = root.join("run.sh");
        fs::hard_link(&outside, &file).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
        write(&root, &file, "new").unwrap();
        assert_eq!(read(&root, &file).unwrap(), "new");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "private");
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".soyeht-write-")
        }));
    }
}

//! Text access for agents. No component of an agent-supplied path follows a symlink. Atomic
//! replacement never truncates another hard link.
use crate::errors::{self, Error, Result};
use soyeht_protocol::limits;
use std::{
    fs::File,
    io::{self, Read, Write},
    path::{Component, Path},
};
use uuid::Uuid;

#[cfg(unix)]
#[path = "filesystem/unix.rs"]
mod system;
#[cfg(windows)]
#[path = "filesystem/windows.rs"]
mod system;

use system::{Attributes, Directory, Name};

/// A finished temporary file, ready to take the place of `target`.
struct Replacement<'a> {
    directory: &'a Directory,
    temporary: &'a Name,
    file: &'a File,
    attributes: &'a Attributes,
    target: &'a Name,
}

fn invalid(message: &'static str) -> Error {
    errors::invalid(message)
}

/// The directory holding `path` (every folder below `root` opened without following a link)
/// and the file's name in it.
fn parent(root: &Path, path: &Path) -> Result<(Directory, Name)> {
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
    let directory = system::open_directory(root.components().chain(parts))?;
    Ok((directory, system::name(leaf)?))
}

pub fn read(root: &Path, path: &Path) -> Result<String> {
    let (directory, leaf) = parent(root, path)?;
    let file = system::open_file(&directory, &leaf)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("text access requires a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limits::ACP_MESSAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limits::ACP_MESSAGE_BYTES {
        return Err(invalid("text file exceeds 1 MiB"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("text file is not UTF-8"))
}

pub fn permits_edit(root: &Path, path: &Path) -> bool {
    let Ok((directory, leaf)) = parent(root, path) else {
        return false;
    };
    match system::open_file(&directory, &leaf) {
        Ok(file) => file.metadata().is_ok_and(|m| m.is_file()),
        Err(Error::Io(error)) => error.kind() == io::ErrorKind::NotFound,
        Err(_) => false,
    }
}

pub fn write(root: &Path, path: &Path, content: &str) -> Result<()> {
    write_with_key(root, path, content, &Uuid::new_v4().to_string())
}

pub fn write_with_key(root: &Path, path: &Path, content: &str, operation: &str) -> Result<()> {
    let operation =
        Uuid::parse_str(operation).map_err(|_| invalid("invalid write operation identity"))?;
    if content.len() > limits::ACP_MESSAGE_BYTES {
        return Err(invalid("text content exceeds 1 MiB"));
    }
    let (directory, leaf) = parent(root, path)?;
    let attributes = match system::open_file(&directory, &leaf) {
        Ok(file) => {
            if !file.metadata()?.is_file() {
                return Err(invalid("text access requires a regular file"));
            }
            system::attributes(&file)?
        }
        Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            system::new_attributes()
        }
        Err(error) => return Err(error),
    };
    let temporary = system::name(format!(".soyeht-write-{operation}").as_ref())?;
    let mut file = system::create_exclusive(&directory, &temporary)?;
    let result = (|| {
        file.write_all(content.as_bytes())?;
        system::replace(&Replacement {
            directory: &directory,
            temporary: &temporary,
            file: &file,
            attributes: &attributes,
            target: &leaf,
        })
    })();
    if result.is_err() {
        system::discard(&directory, &temporary);
    }
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    struct Directory(PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (Directory, PathBuf, PathBuf) {
        let fixture = Directory(std::env::temp_dir().join(format!("sn-files-{}", Uuid::new_v4())));
        fs::create_dir_all(fixture.0.join("work")).unwrap();
        let base = soyeht_platform::fs::canonicalize(&fixture.0).unwrap();
        let root = base.join("work");
        let outside = base.join("outside");
        fs::write(&outside, "private").unwrap();
        (fixture, root, outside)
    }

    /// Neither reading nor writing any of `paths` may succeed.
    fn assert_refused(root: &Path, paths: &[&Path]) {
        for path in paths {
            assert!(read(root, path).is_err());
            assert!(write(root, path, "changed").is_err());
        }
    }

    fn assert_no_temporary_left(root: &Path) {
        assert!(!fs::read_dir(root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".soyeht-write-")
        }));
    }

    #[test]
    fn parent_traversal_and_paths_outside_the_root_are_rejected_without_changing_the_target() {
        let (_fixture, root, outside) = fixture();
        assert_refused(&root, &[&outside, &root.join("../outside")]);
        assert_eq!(fs::read_to_string(&outside).unwrap(), "private");
    }

    #[test]
    fn a_new_file_is_created_and_replaced_atomically() {
        let (_fixture, root, _) = fixture();
        let file = root.join("note.txt");
        write(&root, &file, "first").unwrap();
        assert_eq!(read(&root, &file).unwrap(), "first");
        write(&root, &file, "second").unwrap();
        assert_eq!(read(&root, &file).unwrap(), "second");
        assert!(permits_edit(&root, &file));
        assert!(permits_edit(&root, &root.join("missing.txt")));
        assert_no_temporary_left(&root);
    }

    #[test]
    fn symlinks_and_hard_links_never_change_the_target() {
        use soyeht_platform::fs::symlink;
        let (_fixture, root, outside) = fixture();
        let base = outside.parent().unwrap();
        symlink(base, root.join("link")).unwrap();
        symlink(&outside, root.join("file-link")).unwrap();
        assert_refused(
            &root,
            &[&root.join("link/outside"), &root.join("file-link")],
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "private");
        let file = root.join("run.sh");
        fs::hard_link(&outside, &file).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
        }
        write(&root, &file, "new").unwrap();
        assert_eq!(read(&root, &file).unwrap(), "new");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "private");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert_no_temporary_left(&root);
    }
}

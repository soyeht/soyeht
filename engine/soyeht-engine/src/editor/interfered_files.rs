//! The machine's disk with a point before each read and each write where a test can pause the
//! operation or change the file, as a slow disk or another program would. Compiled only for the
//! unit tests and the `test-hooks` feature, so no shipped engine contains it (ADR 0091).

use super::{FileSystemPort, ProjectFile, ReadOnly, SystemFiles};
use std::{
    io,
    path::{Path, PathBuf},
};

pub trait Interference: Send + Sync {
    /// Runs before a file is read.
    fn before_read(&self) {}

    /// Runs before a file is written. An error fails the write.
    fn before_write(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    /// Whether the file reads as read-only to a write that refuses such a file, as if its owner
    /// had set the bit; the disk is not touched.
    fn read_only(&self, _path: &Path) -> bool {
        false
    }
}

pub struct InterferedFiles<I>(pub I);

impl<I: Interference> FileSystemPort for InterferedFiles<I> {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        SystemFiles.canonicalize(path)
    }

    fn project_files(&self, root: &Path) -> io::Result<Vec<ProjectFile>> {
        SystemFiles.project_files(root)
    }

    fn read_text(&self, path: &Path, limit: usize) -> io::Result<String> {
        self.0.before_read();
        SystemFiles.read_text(path, limit)
    }

    fn read_binary(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        SystemFiles.read_binary(path, limit)
    }

    fn write_text_if_unchanged(
        &self,
        path: &Path,
        expected: &str,
        text: &str,
        read_only: ReadOnly,
    ) -> io::Result<bool> {
        if read_only == ReadOnly::Refuse && self.0.read_only(path) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.0.before_write(path)?;
        SystemFiles.write_text_if_unchanged(path, expected, text, read_only)
    }

    fn creation_target_exists(&self, root: &Path, relative: &Path) -> io::Result<bool> {
        SystemFiles.creation_target_exists(root, relative)
    }

    fn create_empty_text(&self, root: &Path, relative: &Path) -> io::Result<PathBuf> {
        SystemFiles.create_empty_text(root, relative)
    }

    fn create_text_if_absent(
        &self,
        root: &Path,
        relative: &Path,
        text: &str,
    ) -> io::Result<PathBuf> {
        self.0.before_write(&root.join(relative))?;
        SystemFiles.create_text_if_absent(root, relative, text)
    }

    fn create_binary_if_absent_or_matching(
        &self,
        root: &Path,
        relative: &Path,
        bytes: &[u8],
    ) -> io::Result<PathBuf> {
        SystemFiles.create_binary_if_absent_or_matching(root, relative, bytes)
    }
}

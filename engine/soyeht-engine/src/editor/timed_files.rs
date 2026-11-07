//! The editor's file port with the time of every call charged to the hold of the Core lock that
//! makes it (ADR 0147, step 0). A project in a network-mounted folder is slow here and nowhere
//! else, so the slow-hold line says so.

use super::{FileSystemPort, ProjectFile, ReadOnly};
use soyeht_platform::hold_cost::{Cost, timed};
use std::{
    io,
    path::{Path, PathBuf},
};

pub struct TimedFiles(pub Box<dyn FileSystemPort>);

impl FileSystemPort for TimedFiles {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        timed(Cost::File, || self.0.canonicalize(path))
    }

    fn project_files(&self, root: &Path) -> io::Result<Vec<ProjectFile>> {
        timed(Cost::File, || self.0.project_files(root))
    }

    fn read_text(&self, path: &Path, limit: usize) -> io::Result<String> {
        timed(Cost::File, || self.0.read_text(path, limit))
    }

    fn text_matches(&self, path: &Path, expected: &str) -> io::Result<bool> {
        timed(Cost::File, || self.0.text_matches(path, expected))
    }

    fn read_binary(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        timed(Cost::File, || self.0.read_binary(path, limit))
    }

    fn write_text_if_unchanged(
        &self,
        path: &Path,
        expected: &str,
        text: &str,
        read_only: ReadOnly,
    ) -> io::Result<bool> {
        timed(Cost::File, || {
            self.0
                .write_text_if_unchanged(path, expected, text, read_only)
        })
    }

    fn creation_target_exists(&self, root: &Path, relative: &Path) -> io::Result<bool> {
        timed(Cost::File, || self.0.creation_target_exists(root, relative))
    }

    fn create_empty_text(&self, root: &Path, relative: &Path) -> io::Result<PathBuf> {
        timed(Cost::File, || self.0.create_empty_text(root, relative))
    }

    fn create_text_if_absent(&self, root: &Path, rel: &Path, text: &str) -> io::Result<PathBuf> {
        timed(Cost::File, || self.0.create_text_if_absent(root, rel, text))
    }

    fn create_binary_if_absent_or_matching(
        &self,
        root: &Path,
        relative: &Path,
        bytes: &[u8],
    ) -> io::Result<PathBuf> {
        timed(Cost::File, || {
            self.0
                .create_binary_if_absent_or_matching(root, relative, bytes)
        })
    }
}

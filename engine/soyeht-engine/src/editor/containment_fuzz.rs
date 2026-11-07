//! Fuzz targets for the paths the engine resolves inside a workspace: the editor's file
//! resolution and creation, and the agents' text access. Whatever the spelling (`..`, absolute
//! parts, links that lead out, a hard link to a file outside), nothing outside the workspace is
//! read through, written, created or replaced.
use super::service::{FileSystemPort, SystemFiles, resolve_in};
use crate::agents::filesystem;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../../../soyeht-protocol/tests/fuzz_support/mod.rs"]
mod fuzz_support;
use fuzz_support::{Corpus, Rng, fuzz_bytes};

/// The files and folders outside the workspace, with the bytes of each file (or why they could not be read).
type Snapshot = BTreeMap<PathBuf, Result<Vec<u8>, std::io::ErrorKind>>;

const CASES: u64 = 700;

const PIECES: [&str; 18] = [
    ".",
    "..",
    "dir",
    "link_out",
    "link_file",
    "link_in",
    "link_root",
    "secret",
    "file.txt",
    "new.txt",
    "hard",
    "",
    "...",
    "C:",
    "dir/..",
    "link_out/inner",
    "nested/deeper",
    "..\\secret",
];

/// `S/x/y/root` is the workspace; everything else under `S` is outside it.
struct Fixture {
    base: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("soyeht-fuzz-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&base));
        let base = {
            fs::create_dir_all(base.join("x/y/root/dir")).unwrap();
            soyeht_platform::fs::canonicalize(&base).unwrap()
        };
        let root = base.join("x/y/root");
        let outside = base.join("outside");
        fs::create_dir_all(outside.join("inner")).unwrap();
        fs::write(outside.join("secret"), "private").unwrap();
        fs::write(base.join("x/secret"), "private").unwrap();
        fs::write(root.join("file.txt"), "inside").unwrap();
        fs::write(root.join("dir/file.txt"), "inside").unwrap();
        #[cfg(unix)]
        {
            use soyeht_platform::fs::symlink;
            symlink(&outside, root.join("link_out")).unwrap();
            symlink(outside.join("secret"), root.join("link_file")).unwrap();
            symlink(root.join("dir"), root.join("link_in")).unwrap();
            symlink("/", root.join("link_root")).unwrap();
            fs::hard_link(outside.join("secret"), root.join("hard")).unwrap();
        }
        Self { base, root }
    }

    /// Every file and folder outside the workspace, with the bytes of each file.
    fn outside(&self) -> Snapshot {
        let mut found = BTreeMap::new();
        let mut pending = vec![self.base.clone()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                if path == self.root {
                    continue;
                }
                if path.is_dir() && !fs::symlink_metadata(&path).unwrap().is_symlink() {
                    pending.push(path.clone());
                    found.insert(path, Ok(Vec::new()));
                } else {
                    found.insert(path.clone(), fs::read(&path).map_err(|error| error.kind()));
                }
            }
        }
        found
    }

    fn spelling(input: &[u8]) -> String {
        let mut rng = Rng::new(
            input
                .iter()
                .fold(7u64, |a, b| a.wrapping_mul(131) + u64::from(*b)),
        );
        let parts: Vec<&str> = input
            .iter()
            .take(6)
            .map(|byte| PIECES[usize::from(*byte) % PIECES.len()])
            .collect();
        let mut text = parts.join("/");
        if rng.chance(12) {
            text.insert(0, '/');
        }
        text
    }

    fn unchanged_outside(&self, before: &Snapshot, what: &str) {
        assert_eq!(
            &self.outside(),
            before,
            "{what} changed something outside the workspace"
        );
        // The hard link inside the workspace keeps the bytes of the file it names.
        assert!(!std::env::temp_dir().join("new.txt").exists(), "{what}");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.base));
    }
}

fn corpus() -> Corpus<'static> {
    Corpus {
        seeds: &[&[2, 8], &[3, 7], &[1, 1, 7], &[4, 7], &[10], &[6, 6, 7]],
        tokens: &[],
        max_len: 7,
    }
}

#[test]
fn a_workspace_path_never_resolves_or_creates_anything_outside_the_workspace() {
    let fixture = Fixture::new("editor");
    let before = fixture.outside();
    fuzz_bytes("engine-editor-containment", CASES, &corpus(), |input| {
        let relative = Fixture::spelling(input);
        if let Ok(resolved) = resolve_in(&SystemFiles, &fixture.root, &relative) {
            assert!(
                resolved.canonical.starts_with(&fixture.root),
                "{relative:?}"
            );
            assert_ne!(resolved.canonical, fixture.root);
            assert!(
                !resolved.relative.starts_with('/')
                    && resolved.relative.split('/').all(|part| part != ".."),
                "{relative:?} resolved to {:?}",
                resolved.relative
            );
        }
        let path = Path::new(&relative);
        drop(SystemFiles.creation_target_exists(&fixture.root, path));
        if let Ok(created) = SystemFiles.create_empty_text(&fixture.root, path) {
            assert!(
                created.starts_with(&fixture.root),
                "{relative:?} created {created:?}"
            );
        }
        if let Ok(created) = SystemFiles.create_text_if_absent(&fixture.root, path, "pwned") {
            assert!(
                created.starts_with(&fixture.root),
                "{relative:?} created {created:?}"
            );
        }
        drop(SystemFiles.create_binary_if_absent_or_matching(&fixture.root, path, b"pwned"));
        fixture.unchanged_outside(&before, &relative);
    });
}

#[test]
fn an_agents_text_access_follows_no_link_and_never_touches_a_file_outside_the_worktree() {
    let fixture = Fixture::new("agent");
    let before = fixture.outside();
    fuzz_bytes("engine-agent-containment", CASES, &corpus(), |input| {
        let relative = Fixture::spelling(input);
        let path = fixture.root.join(&relative);
        if filesystem::read(&fixture.root, &path).is_ok() {
            // A read that worked went through no link: every part of the path below the
            // workspace is a plain folder or the file itself.
            let mut walked = fixture.root.clone();
            for part in path.strip_prefix(&fixture.root).unwrap().components() {
                walked.push(part);
                assert!(
                    !fs::symlink_metadata(&walked)
                        .unwrap()
                        .file_type()
                        .is_symlink(),
                    "{relative:?} was read through {walked:?}"
                );
            }
        }
        filesystem::permits_edit(&fixture.root, &path);
        drop(filesystem::write(&fixture.root, &path, "pwned"));
        fixture.unchanged_outside(&before, &relative);
        // A path given as absolute outside the workspace is refused outright.
        let outside = fixture.base.join("outside").join("secret");
        assert!(filesystem::write(&fixture.root, &outside, "pwned").is_err());
        assert!(filesystem::read(&fixture.root, &outside).is_err());
    });
}

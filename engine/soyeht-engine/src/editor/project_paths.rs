//! Workspace-confined filesystem operations behind the project panel.
//!
//! Every path arrives relative to the workspace root and is checked twice: its
//! components must be plain names (no `..`, no root, no `.`), and each directory
//! it passes through must canonicalize inside the canonical root, so a symbolic
//! link cannot carry an operation out of the workspace. The final component is
//! never followed: renaming or deleting a link acts on the link itself.

use super::project_files::is_save_temporary;
use soyeht_platform::fs::rename_no_replace;
use soyeht_protocol::{ProjectEntry, ProjectEntryKind};
use std::{
    collections::{BTreeSet, hash_map::DefaultHasher},
    ffi::{OsStr, OsString},
    fs,
    hash::{Hash, Hasher},
    io,
    path::{Component, Path, PathBuf},
};

/// Entries the panel never shows, as Zed's default `file_scan_exclusions`.
const EXCLUDED_NAMES: &[&str] = &[".git", ".hg", ".svn", ".DS_Store"];

/// Entries the panel never shows or accepts: repository metadata and the
/// engine's own atomic-save temporary files.
pub(super) fn is_excluded(name: &OsStr) -> bool {
    is_save_temporary(name)
        || name
            .to_str()
            .is_some_and(|name| EXCLUDED_NAMES.contains(&name))
}

pub(crate) struct ProjectRoot {
    root: PathBuf,
}

#[derive(Debug)]
pub(crate) struct Listing {
    pub entries: Vec<ProjectEntry>,
    pub truncated: bool,
}

impl ProjectRoot {
    pub fn open(workspace: &Path) -> io::Result<Self> {
        Ok(Self {
            root: soyeht_platform::fs::canonicalize(workspace)?,
        })
    }

    /// The canonical workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// An existing directory; the empty path is the workspace root.
    pub fn directory(&self, relative: &str) -> io::Result<PathBuf> {
        let names = names(relative, true)?;
        let mut current = self.root.clone();
        for name in names {
            current = self.contained(&current.join(name))?;
        }
        if !fs::metadata(&current)?.is_dir() {
            return Err(invalid("project path is not a folder"));
        }
        Ok(current)
    }

    /// An existing entry below the root, with its last component unresolved.
    pub fn entry(&self, relative: &str) -> io::Result<PathBuf> {
        let (parent, name) = split(relative)?;
        let path = self.directory(&parent)?.join(name);
        fs::symlink_metadata(&path)?;
        Ok(path)
    }

    /// Lists one directory level, folders first, ignoring `.gitignore`d
    /// entries unless `show_ignored` is set.
    pub fn list(&self, relative: &str, show_ignored: bool, limit: usize) -> io::Result<Listing> {
        let directory = self.directory(relative)?;
        let prefix = names(relative, true)?
            .iter()
            .map(|name| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let visible = unignored_names(&directory)?;
        let mut entries = Vec::new();
        for item in fs::read_dir(&directory)? {
            let item = item?;
            let name = item.file_name();
            let Some(text) = name.to_str() else { continue };
            if is_excluded(&name) {
                continue;
            }
            let ignored = !visible.contains(&name);
            if ignored && !show_ignored {
                continue;
            }
            let Some(kind) = self.kind(&item.path()) else {
                continue;
            };
            let path = if prefix.is_empty() {
                text.to_owned()
            } else {
                format!("{prefix}/{text}")
            };
            entries.push(ProjectEntry {
                name: text.to_owned(),
                path,
                kind,
                ignored,
            });
        }
        entries.sort_by(|left, right| {
            (left.kind != ProjectEntryKind::Directory)
                .cmp(&(right.kind != ProjectEntryKind::Directory))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.name.cmp(&right.name))
        });
        let truncated = entries.len() > limit;
        entries.truncate(limit);
        Ok(Listing { entries, truncated })
    }

    /// Creates a file or folder, and any missing folders above it.
    pub fn create(&self, relative: &str, kind: ProjectEntryKind) -> io::Result<ProjectEntry> {
        let (parent, name) = split(relative)?;
        let target = self.ensure_directory(&parent)?.join(&name);
        match kind {
            ProjectEntryKind::File => {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)?
                    .sync_all()?;
            }
            ProjectEntryKind::Directory => fs::create_dir(&target)?,
        }
        Ok(ProjectEntry {
            name: name.to_string_lossy().into_owned(),
            path: normalized(relative)?,
            kind,
            ignored: false,
        })
    }

    /// Renames without replacing anything already at `to`.
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let source = self.entry(from)?;
        if normalized(to)?.starts_with(&format!("{}/", normalized(from)?)) {
            return Err(invalid("a folder cannot move inside itself"));
        }
        let (parent, name) = split(to)?;
        let target = self.ensure_directory(&parent)?.join(name);
        let source_metadata = fs::symlink_metadata(&source)?;
        if source_metadata.is_dir() && target.starts_with(&source) {
            return Err(invalid("a folder cannot move inside itself"));
        }
        match fs::symlink_metadata(&target) {
            Ok(_) if same_entry(&source, &target) => {
                // A case-only rename on a case-insensitive volume names the same entry.
                fs::rename(&source, &target)
            }
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "an entry with that name already exists",
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                rename_no_replace(&source, &target)
            }
            Err(error) => Err(error),
        }
    }

    /// Deletes permanently; a folder goes with everything inside it.
    pub fn remove(&self, relative: &str) -> io::Result<()> {
        let path = self.entry(relative)?;
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        }
    }

    /// Moves an entry to the system trash.
    pub fn trash(&self, relative: &str) -> io::Result<()> {
        let path = self.entry(relative)?;
        super::trash::move_to_trash(&path, fs::symlink_metadata(&path)?.is_dir())
    }

    /// A cheap summary of one directory level, for change polling.
    pub fn fingerprint(&self, relative: &str) -> Option<u64> {
        // ignored: a folder that cannot be read has no fingerprint, so the poll sees no change
        self.read_fingerprint(relative).ok()
    }

    fn read_fingerprint(&self, relative: &str) -> io::Result<u64> {
        let mut names = BTreeSet::new();
        for item in fs::read_dir(self.directory(relative)?)? {
            let item = item?;
            if is_excluded(&item.file_name()) {
                continue;
            }
            names.insert((item.file_name(), item.file_type()?.is_dir()));
        }
        let mut hasher = DefaultHasher::new();
        names.hash(&mut hasher);
        Ok(hasher.finish())
    }

    fn contained(&self, path: &Path) -> io::Result<PathBuf> {
        let canonical = soyeht_platform::fs::canonicalize(path)?;
        if !canonical.starts_with(&self.root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "project path leaves the workspace",
            ));
        }
        Ok(canonical)
    }

    fn kind(&self, path: &Path) -> Option<ProjectEntryKind> {
        // ignored: an entry that is gone, or a link that leaves the project, has no kind
        let metadata = self.metadata_of(path).ok()?;
        if metadata.is_dir() {
            Some(ProjectEntryKind::Directory)
        } else if metadata.is_file() {
            Some(ProjectEntryKind::File)
        } else {
            None
        }
    }

    /// The metadata of `path`, followed through a link that stays inside the project.
    fn metadata_of(&self, path: &Path) -> io::Result<fs::Metadata> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            fs::metadata(self.contained(path)?)
        } else {
            Ok(metadata)
        }
    }

    fn ensure_directory(&self, relative: &str) -> io::Result<PathBuf> {
        let mut current = self.root.clone();
        for name in names(relative, true)? {
            let next = current.join(name);
            match fs::create_dir(&next) {
                Ok(()) => soyeht_platform::fs::sync_directory(&current)?,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            current = self.contained(&next)?;
            if !fs::metadata(&current)?.is_dir() {
                return Err(invalid("a parent of the project path is a file"));
            }
        }
        Ok(current)
    }
}

/// Splits a workspace-relative path into plain names. The empty path is the
/// root only where `allow_root` says so.
fn names(relative: &str, allow_root: bool) -> io::Result<Vec<&OsStr>> {
    if relative.is_empty() {
        return if allow_root {
            Ok(Vec::new())
        } else {
            Err(invalid("project path is empty"))
        };
    }
    let path = Path::new(relative);
    let mut names = Vec::new();
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("project path must stay inside the workspace"));
        };
        if is_excluded(name) {
            return Err(invalid("project path names engine or repository metadata"));
        }
        names.push(name);
    }
    if names.is_empty() || relative.ends_with('/') || relative.contains("//") {
        return Err(invalid("project path is not normalized"));
    }
    Ok(names)
}

fn split(relative: &str) -> io::Result<(String, OsString)> {
    let mut names = names(relative, false)?;
    let name = names
        .pop()
        .ok_or_else(|| invalid("project path is empty"))?
        .to_os_string();
    let parent = names
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    Ok((parent, name))
}

pub(super) fn normalized(relative: &str) -> io::Result<String> {
    Ok(names(relative, false)?
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/"))
}

/// Names in `directory` that survive `.gitignore`, `.ignore` and global git
/// excludes, including rules inherited from parent folders.
fn unignored_names(directory: &Path) -> io::Result<BTreeSet<OsString>> {
    let mut builder = ignore::WalkBuilder::new(directory);
    builder
        .standard_filters(true)
        .hidden(false)
        .parents(true)
        .follow_links(false)
        .max_depth(Some(1));
    let mut names = BTreeSet::new();
    for entry in builder.build() {
        let entry = entry.map_err(|error| io::Error::other(error.to_string()))?;
        if entry.depth() == 1 {
            names.insert(entry.file_name().to_os_string());
        }
    }
    Ok(names)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}

/// Whether both paths name the same entry, as a case-only rename on a case-insensitive volume
/// does.
#[cfg(unix)]
fn same_entry(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::symlink_metadata(left), fs::symlink_metadata(right)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
        _ => false,
    }
}

#[cfg(windows)]
fn same_entry(left: &Path, right: &Path) -> bool {
    matches!(
        (soyeht_platform::fs::canonicalize(left), soyeht_platform::fs::canonicalize(right)),
        (Ok(left), Ok(right)) if left == right
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct Sandbox {
        base: PathBuf,
        root: PathBuf,
        outside: PathBuf,
    }

    impl Sandbox {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!("sn-project-{}", Uuid::new_v4()));
            let root = base.join("workspace");
            let outside = base.join("outside");
            fs::create_dir_all(root.join(".git")).unwrap();
            fs::create_dir_all(root.join("src/nested")).unwrap();
            fs::create_dir_all(&outside).unwrap();
            fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
            fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
            fs::write(root.join("README.md"), "readme").unwrap();
            fs::write(root.join("debug.log"), "noise").unwrap();
            fs::create_dir_all(root.join("target")).unwrap();
            fs::write(outside.join("secret.txt"), "secret").unwrap();
            soyeht_platform::fs::symlink(&outside, root.join("escape")).unwrap();
            soyeht_platform::fs::symlink(outside.join("secret.txt"), root.join("secret-link"))
                .unwrap();
            soyeht_platform::fs::symlink(root.join("src"), root.join("src-link")).unwrap();
            Self {
                base,
                root,
                outside,
            }
        }

        fn project(&self) -> ProjectRoot {
            ProjectRoot::open(&self.root).unwrap()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    fn paths(listing: &Listing) -> Vec<(String, bool)> {
        listing
            .entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.ignored))
            .collect()
    }

    #[test]
    fn listing_is_lazy_sorted_ignore_aware_and_never_shows_git_or_escaping_links() {
        let sandbox = Sandbox::new();
        let project = sandbox.project();
        let root = project.list("", false, 100).unwrap();
        assert_eq!(
            paths(&root),
            vec![
                ("src".into(), false),
                ("src-link".into(), false),
                (".gitignore".into(), false),
                ("README.md".into(), false),
            ]
        );
        assert_eq!(root.entries[1].kind, ProjectEntryKind::Directory);
        let all = project.list("", true, 100).unwrap();
        assert!(paths(&all).contains(&("target".into(), true)));
        assert!(paths(&all).contains(&("debug.log".into(), true)));
        assert!(!paths(&all).iter().any(|(path, _)| path == ".git"));
        assert!(
            !paths(&all)
                .iter()
                .any(|(path, _)| path.starts_with("escape"))
        );
        assert!(!paths(&all).iter().any(|(path, _)| path == "secret-link"));
        let nested = project.list("src", false, 100).unwrap();
        assert_eq!(
            paths(&nested),
            vec![("src/nested".into(), false), ("src/main.rs".into(), false)]
        );
        let bounded = project.list("", true, 2).unwrap();
        assert!(bounded.truncated);
        assert_eq!(bounded.entries.len(), 2);
    }

    #[test]
    fn every_operation_rejects_parent_components_and_symlink_escapes() {
        let sandbox = Sandbox::new();
        let project = sandbox.project();
        for path in [
            "../outside",
            "src/../../outside",
            "/etc",
            "./src",
            "src//main.rs",
        ] {
            assert!(project.list(path, false, 10).is_err(), "{path}");
            assert!(
                project.create(path, ProjectEntryKind::File).is_err(),
                "{path}"
            );
            assert!(project.remove(path).is_err(), "{path}");
        }
        assert_eq!(
            project.list("escape", false, 10).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(
            project
                .create("escape/planted.txt", ProjectEntryKind::File)
                .is_err()
        );
        assert!(!sandbox.outside.join("planted.txt").exists());
        assert!(project.rename("README.md", "escape/README.md").is_err());
        assert!(project.rename("escape/secret.txt", "stolen.txt").is_err());
        assert!(project.remove("escape/secret.txt").is_err());
        assert!(
            project
                .create(".git/hooks", ProjectEntryKind::Directory)
                .is_err()
        );
        assert!(project.remove(".git").is_err());
        assert!(sandbox.outside.join("secret.txt").exists());
        assert!(sandbox.root.join("README.md").exists());
        assert!(sandbox.root.join(".git").exists());

        // Deleting a link removes the link, never what it points at.
        project.remove("secret-link").unwrap();
        assert!(sandbox.outside.join("secret.txt").exists());
    }

    #[test]
    fn create_rename_and_delete_work_inside_the_workspace() {
        let sandbox = Sandbox::new();
        let project = sandbox.project();
        let file = project
            .create("src/new/module.rs", ProjectEntryKind::File)
            .unwrap();
        assert_eq!(file.path, "src/new/module.rs");
        assert_eq!(file.kind, ProjectEntryKind::File);
        assert!(sandbox.root.join("src/new/module.rs").is_file());
        assert_eq!(
            project
                .create("src/new/module.rs", ProjectEntryKind::File)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        project.create("docs", ProjectEntryKind::Directory).unwrap();
        assert!(sandbox.root.join("docs").is_dir());

        project
            .rename("src/new/module.rs", "src/new/renamed.rs")
            .unwrap();
        assert!(sandbox.root.join("src/new/renamed.rs").is_file());
        assert_eq!(
            project
                .rename("README.md", "src/main.rs")
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            fs::read_to_string(sandbox.root.join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert!(project.rename("src", "src/inner/src").is_err());
        project.rename("src/new", "docs/new").unwrap();
        assert!(sandbox.root.join("docs/new/renamed.rs").is_file());
        project.rename("README.md", "readme.md").unwrap();
        assert!(
            project
                .list("", false, 10)
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.name == "readme.md")
        );

        let before = project.fingerprint("docs");
        project.remove("docs/new").unwrap();
        assert!(!sandbox.root.join("docs/new").exists());
        assert_ne!(project.fingerprint("docs"), before);
        project.remove("readme.md").unwrap();
        assert!(!sandbox.root.join("readme.md").exists());
        assert_eq!(
            project.remove("readme.md").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(project.remove("").is_err());
    }
}

//! One directory of skill folders: `<root>/<name>/SKILL.md`.

use crate::{Error, MANIFEST, Manifest, Problem, Result, manifest};
use sha2::{Digest, Sha256};
use soyeht_protocol::limits::{SKILL_BYTES, SKILL_ENTRIES};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

/// One folder of the store, valid or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The folder name, which a valid skill's `name` equals.
    pub name: String,
    pub directory: PathBuf,
    /// SHA-256 of the manifest bytes, when they could be read.
    pub revision: Option<String>,
    pub manifest: std::result::Result<Manifest, Problem>,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Skill folders by name, at most `SKILL_ENTRIES`. A missing root is an
    /// empty store; hidden folders and loose files are not skills.
    pub fn list(&self) -> io::Result<Vec<Entry>> {
        let reader = match fs::read_dir(&self.root) {
            Ok(reader) => reader,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut names = Vec::new();
        for entry in reader {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !name.starts_with('.') && entry.path().is_dir() {
                names.push(name);
            }
        }
        names.sort();
        names.truncate(SKILL_ENTRIES);
        Ok(names.into_iter().map(|name| self.entry(name).0).collect())
    }

    /// The folder and its manifest text.
    pub fn read(&self, name: &str) -> Result<(Entry, String)> {
        self.existing(name)?;
        match self.entry(name.to_owned()) {
            (entry, Some(text)) => Ok((entry, text)),
            (entry, None) => Err(Error::Invalid(
                entry.manifest.err().unwrap_or(Problem::Unreadable),
            )),
        }
    }

    /// A new skill folder holding only `SKILL.md`.
    pub fn create(&self, name: &str, description: &str, instructions: &str) -> Result<Entry> {
        let directory = self.directory(name)?;
        let text = manifest::render(name, description, instructions);
        manifest::parse(name, &text).map_err(Error::Invalid)?;
        fs::create_dir_all(&self.root)?;
        match fs::create_dir(&directory) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(Error::AlreadyExists);
            }
            result => result?,
        }
        write_atomically(&directory.join(MANIFEST), &text)?;
        Ok(self.entry(name.to_owned()).0)
    }

    /// Replaces `SKILL.md` when it still has `base_revision`. The new text
    /// must be a valid manifest for the same folder.
    pub fn write(&self, name: &str, text: &str, base_revision: &str) -> Result<Entry> {
        self.existing(name)?;
        if text.len() > SKILL_BYTES {
            return Err(Error::Invalid(Problem::TooLarge));
        }
        manifest::parse(name, text).map_err(Error::Invalid)?;
        let (current, _) = self.entry(name.to_owned());
        if current.revision.as_deref() != Some(base_revision) {
            return Err(Error::StaleRevision);
        }
        write_atomically(&self.directory(name)?.join(MANIFEST), text)?;
        Ok(self.entry(name.to_owned()).0)
    }

    /// Removes the folder with everything in it. A folder that is a
    /// symbolic link loses only the link.
    pub fn delete(&self, name: &str) -> Result<()> {
        let directory = self.existing(name)?;
        if fs::symlink_metadata(&directory)?.file_type().is_symlink() {
            fs::remove_file(&directory)?;
        } else {
            fs::remove_dir_all(&directory)?;
        }
        Ok(())
    }

    /// Folder names come from people and agents, so any spelling is
    /// addressable as long as it stays one plain component of the root.
    fn directory(&self, name: &str) -> Result<PathBuf> {
        let plain = !name.is_empty()
            && !name.starts_with('.')
            && !name.contains(['/', '\\', '\0'])
            && name.chars().count() <= manifest::NAME_CHARS * 2;
        if !plain {
            return Err(Error::InvalidName);
        }
        Ok(self.root.join(name))
    }

    fn existing(&self, name: &str) -> Result<PathBuf> {
        let directory = self.directory(name)?;
        if directory.is_dir() {
            Ok(directory)
        } else {
            Err(Error::NotFound)
        }
    }

    fn entry(&self, name: String) -> (Entry, Option<String>) {
        let directory = self.root.join(&name);
        let (revision, text, manifest) = match read_manifest(&directory.join(MANIFEST)) {
            Ok((revision, text)) => {
                let manifest = manifest::parse(&name, &text);
                (Some(revision), Some(text), manifest)
            }
            Err(problem) => (None, None, Err(problem)),
        };
        let entry = Entry {
            name,
            directory,
            revision,
            manifest,
        };
        (entry, text)
    }
}

fn read_manifest(path: &Path) -> std::result::Result<(String, String), Problem> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(Problem::MissingManifest);
        }
        // `Unreadable` is the typed problem the skill list shows.
        Err(_) => return Err(Problem::Unreadable),
    };
    if metadata.len() > SKILL_BYTES as u64 {
        return Err(Problem::TooLarge);
    }
    let bytes = fs::read(path).map_err(|_| Problem::Unreadable)?;
    let revision = format!("{:x}", Sha256::digest(&bytes));
    let text = String::from_utf8(bytes).map_err(|_| Problem::NotText)?;
    Ok((revision, text))
}

fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    soyeht_platform::fs::write_atomic(
        path,
        text.as_bytes(),
        soyeht_platform::fs::WriteMode::KeepPermissions,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        let root = std::env::temp_dir().join(format!("soyeht-skills-{}", uuid::Uuid::new_v4()));
        Store::new(root)
    }

    #[test]
    fn create_read_write_and_delete_one_skill() {
        let store = store();
        assert!(store.list().unwrap().is_empty());
        let created = store
            .create("notes", "Keeps notes.", "Write notes.")
            .unwrap();
        assert_eq!(
            created.manifest.as_ref().unwrap().description,
            "Keeps notes."
        );
        assert!(matches!(
            store.create("notes", "Again", ""),
            Err(Error::AlreadyExists)
        ));
        let (entry, text) = store.read("notes").unwrap();
        assert_eq!(entry, created);
        let edited = text.replace("Write notes.", "Write short notes.");
        assert!(matches!(
            store.write("notes", &edited, "stale"),
            Err(Error::StaleRevision)
        ));
        let revision = entry.revision.unwrap();
        let invalid = edited.replace("name: notes", "name: other");
        assert!(matches!(
            store.write("notes", &invalid, &revision),
            Err(Error::Invalid(Problem::NameMismatch))
        ));
        let written = store.write("notes", &edited, &revision).unwrap();
        assert_ne!(written.revision.as_deref(), Some(revision.as_str()));
        store.delete("notes").unwrap();
        assert!(matches!(store.read("notes"), Err(Error::NotFound)));
        fs::remove_dir_all(store.root()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_write_keeps_the_manifest_permissions_and_leaves_no_temporary_file() {
        use std::os::unix::fs::PermissionsExt;
        let store = store();
        store.create("notes", "Keeps notes.", "").unwrap();
        let manifest = store.root().join("notes").join(MANIFEST);
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o640)).unwrap();
        let (entry, text) = store.read("notes").unwrap();
        store
            .write(
                "notes",
                &text.replace("Keeps", "Holds"),
                &entry.revision.unwrap(),
            )
            .unwrap();
        assert_eq!(
            fs::metadata(&manifest).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let names: Vec<_> = fs::read_dir(store.root().join("notes"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from(MANIFEST)]);
        fs::remove_dir_all(store.root()).unwrap();
    }

    #[test]
    fn listing_keeps_broken_folders_with_their_problem() {
        let store = store();
        store.create("good", "Fine.", "").unwrap();
        fs::create_dir_all(store.root().join("empty")).unwrap();
        fs::create_dir_all(store.root().join("Bad Name")).unwrap();
        fs::write(
            store.root().join("Bad Name").join(MANIFEST),
            "---\nname: Bad Name\ndescription: x\n---\n",
        )
        .unwrap();
        fs::create_dir_all(store.root().join(".hidden")).unwrap();
        fs::write(store.root().join("loose.md"), "").unwrap();
        let listed: Vec<_> = store
            .list()
            .unwrap()
            .into_iter()
            .map(|entry| (entry.name, entry.manifest.err()))
            .collect();
        assert_eq!(
            listed,
            [
                ("Bad Name".to_owned(), Some(Problem::InvalidName)),
                ("empty".to_owned(), Some(Problem::MissingManifest)),
                ("good".to_owned(), None),
            ]
        );
        assert!(matches!(store.read("../good"), Err(Error::InvalidName)));
        assert!(matches!(
            store.read("Bad Name"),
            Ok((
                Entry {
                    manifest: Err(Problem::InvalidName),
                    ..
                },
                _
            ))
        ));
        fs::remove_dir_all(store.root()).unwrap();
    }
}

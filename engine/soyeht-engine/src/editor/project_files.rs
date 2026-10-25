//! Project file enumeration shared by search, symbols and writing sections.

use soyeht_protocol::EditorLanguage;
use std::{
    ffi::OsStr,
    fs, io,
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

/// A fresh name for an atomic save's temporary file.
pub(super) fn save_temporary_name() -> String {
    format!(
        "{}{}",
        crate::immutable_file::SAVE_TEMPORARY_PREFIX,
        Uuid::new_v4()
    )
}

/// True for the engine's own temporary files (an atomic save's and a publish's), which no search,
/// listing or file event may ever show.
pub(super) fn is_save_temporary(name: &OsStr) -> bool {
    crate::immutable_file::is_engine_temporary(name)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFile {
    pub canonical: PathBuf,
    pub relative: String,
    pub byte_count: u64,
    pub modified_ms: i64,
}

/// A path below a root as the wire names it: its normal components joined with `/`, on every
/// system. `None` when the path is empty, is not UTF-8 or has a component that is not a name.
pub(crate) fn wire_path(relative: &Path) -> Option<String> {
    let names = relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (!names.is_empty()).then(|| names.join("/"))
}

/// [`wire_path`], or an error that says why the path has no wire form.
pub(crate) fn checked_wire_path(relative: &Path) -> crate::errors::Result<String> {
    wire_path(relative).ok_or_else(|| {
        if relative.to_str().is_none() {
            crate::errors::invalid("File path must be UTF-8")
        } else {
            crate::errors::invalid("File path must name a file inside the workspace")
        }
    })
}

/// The closed editor language of a path; unknown extensions are plain text.
pub fn language(path: &Path) -> EditorLanguage {
    match path.extension().and_then(|value| value.to_str()) {
        Some("rs") => EditorLanguage::Rust,
        Some("swift") => EditorLanguage::Swift,
        Some("ts" | "tsx" | "js" | "jsx") => EditorLanguage::Typescript,
        Some("py") => EditorLanguage::Python,
        Some("json") => EditorLanguage::Json,
        Some("md" | "markdown") => EditorLanguage::Markdown,
        Some("sh" | "bash" | "zsh") => EditorLanguage::Shell,
        _ => EditorLanguage::PlainText,
    }
}

pub(crate) fn enumerate_project_files(root: &Path) -> io::Result<Vec<ProjectFile>> {
    let root = soyeht_platform::fs::canonicalize(root)?;
    let mut builder = ignore::WalkBuilder::new(&root);
    builder
        .standard_filters(true)
        .hidden(false)
        .follow_links(false)
        .filter_entry(|entry| {
            !is_source_metadata(entry.path()) && !is_save_temporary(entry.file_name())
        });
    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(walk_error)?;
        let Some(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_file() || kind.is_symlink() {
            continue;
        }
        let canonical = soyeht_platform::fs::canonicalize(entry.path())?;
        if !canonical.starts_with(&root) {
            continue;
        }
        let relative = canonical.strip_prefix(&root).ok().and_then(wire_path);
        if let Some(relative) = relative {
            let metadata = fs::metadata(&canonical)?;
            let modified_ms = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|value| i64::try_from(value.as_millis()).ok())
                .unwrap_or(0);
            files.push(ProjectFile {
                canonical,
                relative,
                byte_count: metadata.len(),
                modified_ms,
            });
        }
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

fn is_source_metadata(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | ".hg" | ".svn"))
}

fn walk_error(error: ignore::Error) -> io::Error {
    let kind = error
        .io_error()
        .map(io::Error::kind)
        .unwrap_or(io::ErrorKind::InvalidData);
    io::Error::new(kind, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn a_path_with_a_parent_component_is_not_called_non_utf8() {
        let error = checked_wire_path(Path::new("a/../b"))
            .unwrap_err()
            .to_string();
        assert!(!error.contains("UTF-8"), "{error}");
        assert!(error.contains("inside the workspace"), "{error}");
        assert_eq!(checked_wire_path(Path::new("a/b.rs")).unwrap(), "a/b.rs");
    }

    #[test]
    fn project_enumeration_honors_ignores_and_never_follows_metadata_or_symlinks() {
        let root = std::env::temp_dir().join(format!("sn-files-{}", Uuid::new_v4()));
        let outside = std::env::temp_dir().join(format!("sn-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("ignored-dir")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join(".svn")).unwrap();
        fs::write(root.join(".gitignore"), "ignored.txt\nignored-dir/\n").unwrap();
        fs::write(root.join("src/main.rs"), "visible").unwrap();
        fs::write(root.join("ignored.txt"), "hidden").unwrap();
        fs::write(root.join("ignored-dir/data.txt"), "hidden").unwrap();
        fs::write(root.join(".git/secret"), "hidden").unwrap();
        fs::write(root.join(".svn/secret"), "hidden").unwrap();
        fs::write(&outside, "outside").unwrap();
        soyeht_platform::fs::symlink(&outside, root.join("src/link.txt")).unwrap();

        let files = enumerate_project_files(&root).unwrap();
        let paths = files
            .into_iter()
            .map(|file| file.relative)
            .collect::<Vec<_>>();
        assert_eq!(paths, vec![".gitignore", "src/main.rs"]);

        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn language_mapping_is_closed_and_unknown_files_are_plain_text() {
        assert_eq!(language(Path::new("main.rs")), EditorLanguage::Rust);
        assert_eq!(language(Path::new("view.tsx")), EditorLanguage::Typescript);
        assert_eq!(
            language(Path::new("notes.unknown")),
            EditorLanguage::PlainText
        );
    }
}

//! Project file enumeration shared by search, symbols and writing sections.

use soyeht_protocol::EditorLanguage;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFile {
    pub canonical: PathBuf,
    pub relative: String,
    pub byte_count: u64,
    pub modified_ms: i64,
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
    let root = fs::canonicalize(root)?;
    let mut builder = ignore::WalkBuilder::new(&root);
    builder
        .standard_filters(true)
        .hidden(false)
        .follow_links(false)
        .filter_entry(|entry| !is_source_metadata(entry.path()));
    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(walk_error)?;
        let Some(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_file() || kind.is_symlink() {
            continue;
        }
        let canonical = fs::canonicalize(entry.path())?;
        if !canonical.starts_with(&root) {
            continue;
        }
        let relative = canonical
            .strip_prefix(&root)
            .ok()
            .and_then(Path::to_str)
            .filter(|path| !path.is_empty())
            .map(str::to_owned);
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
        std::os::unix::fs::symlink(&outside, root.join("src/link.txt")).unwrap();

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

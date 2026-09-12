use crate::{errors, errors::Result};
use soyeht_editor::SyntaxDocument;
use soyeht_protocol::{EditorLanguage, EditorLine};
use soyeht_store::editor::EditorFile;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

pub trait FileSystemPort: Send {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
    fn read_text(&self, path: &Path, limit: usize) -> io::Result<String>;
    fn write_text(&self, path: &Path, text: &str) -> io::Result<()>;
}

#[derive(Default)]
pub struct SystemFiles;

impl FileSystemPort for SystemFiles {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        path.canonicalize()
    }

    fn read_text(&self, path: &Path, limit: usize) -> io::Result<String> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() || metadata.len() > limit as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file is not regular text within the editor size limit",
            ));
        }
        let bytes = fs::read(path)?;
        if bytes.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file exceeds the editor size limit",
            ));
        }
        String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "file is not UTF-8"))
    }

    fn write_text(&self, path: &Path, text: &str) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "file has no parent directory")
        })?;
        let temporary = parent.join(format!(".soyeht-save-{}", Uuid::new_v4()));
        let permissions = fs::metadata(path)?.permissions();
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = (|| {
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::set_permissions(&temporary, permissions)?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

struct OpenDocument {
    file: EditorFile,
    syntax: Option<SyntaxDocument>,
}

pub struct Service {
    files: Box<dyn FileSystemPort>,
    open: BTreeMap<String, OpenDocument>,
}

pub struct ResolvedPath {
    pub canonical: PathBuf,
    pub relative: String,
}

impl Service {
    pub fn new(files: Box<dyn FileSystemPort>) -> Self {
        Self {
            files,
            open: BTreeMap::new(),
        }
    }

    pub fn restore(&mut self, file: EditorFile) {
        self.open
            .insert(file.file_id.clone(), OpenDocument { file, syntax: None });
    }

    pub fn resolve(&self, workspace_root: &Path, requested: &str) -> Result<ResolvedPath> {
        let requested_path = Path::new(requested);
        if requested_path.is_absolute()
            || requested_path.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(errors::invalid("File path must stay within the workspace"));
        }
        let root = self
            .files
            .canonicalize(workspace_root)
            .map_err(errors::io)?;
        let target = self
            .files
            .canonicalize(&root.join(requested_path))
            .map_err(errors::io)?;
        if target == root || !target.starts_with(&root) {
            return Err(errors::invalid("File path escapes the workspace"));
        }
        let relative = target
            .strip_prefix(&root)
            .map_err(|_| errors::invalid("File path escapes the workspace"))?
            .to_str()
            .ok_or_else(|| errors::invalid("File path must be UTF-8"))?
            .to_owned();
        Ok(ResolvedPath {
            canonical: target,
            relative,
        })
    }

    pub fn read_disk(&self, path: &Path) -> Result<String> {
        self.files
            .read_text(path, soyeht_protocol::limits::EDITOR_HISTORY_BYTES)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::InvalidData {
                    errors::invalid(error.to_string())
                } else {
                    errors::io(error)
                }
            })
    }

    pub fn insert(&mut self, file: EditorFile) {
        self.open
            .insert(file.file_id.clone(), OpenDocument { file, syntax: None });
    }

    pub fn stored(&self, file_id: &str) -> Result<&EditorFile> {
        self.open
            .get(file_id)
            .map(|item| &item.file)
            .ok_or_else(|| errors::not_found("Open file not found"))
    }

    pub fn stored_mut(&mut self, file_id: &str) -> Result<&mut EditorFile> {
        self.open
            .get_mut(file_id)
            .map(|item| &mut item.file)
            .ok_or_else(|| errors::not_found("Open file not found"))
    }

    pub fn viewport(
        &mut self,
        file_id: &str,
        text: &str,
        language: EditorLanguage,
        from_line: usize,
        line_count: usize,
    ) -> Result<Vec<EditorLine>> {
        let item = self
            .open
            .get_mut(file_id)
            .ok_or_else(|| errors::not_found("Open file not found"))?;
        let needs_document = item
            .syntax
            .as_ref()
            .is_none_or(|document| document.buffer().text() != text);
        if needs_document {
            item.syntax = Some(SyntaxDocument::new(text, language)?);
        }
        let document = item
            .syntax
            .as_ref()
            .ok_or_else(|| errors::internal("Editor document was not initialized"))?;
        soyeht_editor::project(document, from_line, line_count).map_err(Into::into)
    }

    pub fn remove_pane(&mut self, pane_id: &str) {
        self.open.retain(|_, item| item.file.pane_id != pane_id);
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeFiles {
        canonical: BTreeMap<PathBuf, PathBuf>,
        contents: BTreeMap<PathBuf, String>,
        writes: Arc<Mutex<Vec<(PathBuf, String)>>>,
    }

    impl FileSystemPort for FakeFiles {
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            self.canonical
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "fixture path"))
        }
        fn read_text(&self, path: &Path, limit: usize) -> io::Result<String> {
            let text = self
                .contents
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "fixture file"))?;
            (text.len() <= limit)
                .then_some(text)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "fixture too large"))
        }
        fn write_text(&self, path: &Path, text: &str) -> io::Result<()> {
            self.writes
                .lock()
                .map_err(|_| io::Error::other("fixture lock"))?
                .push((path.into(), text.into()));
            Ok(())
        }
    }

    #[test]
    fn resolver_rejects_absolute_parent_and_symlink_escape_before_opening() {
        let root = PathBuf::from("/repo");
        let mut files = FakeFiles::default();
        files.canonical.insert(root.clone(), root.clone());
        files
            .canonical
            .insert(root.join("src/main.rs"), root.join("src/main.rs"));
        files
            .canonical
            .insert(root.join("link.rs"), PathBuf::from("/outside/secret.rs"));
        let service = Service::new(Box::new(files));

        let resolved = service.resolve(&root, "src/main.rs").unwrap();
        assert_eq!(resolved.canonical, root.join("src/main.rs"));
        assert_eq!(resolved.relative, "src/main.rs");
        assert!(service.resolve(&root, "/outside/secret.rs").is_err());
        assert!(service.resolve(&root, "../secret.rs").is_err());
        assert!(service.resolve(&root, "link.rs").is_err());
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

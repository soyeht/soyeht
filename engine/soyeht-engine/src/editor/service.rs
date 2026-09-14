use crate::{errors, errors::Result};
use soyeht_editor::{SyntaxDocument, TextBuffer};
use soyeht_protocol::{EditorLanguage, EditorLine, FileAuthor, TextEdit};
use soyeht_store::editor::EditorFile;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

pub trait FileSystemPort: Send {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
    fn project_files(&self, root: &Path) -> io::Result<Vec<ProjectFile>>;
    fn read_text(&self, path: &Path, limit: usize) -> io::Result<String>;
    fn write_text_if_unchanged(&self, path: &Path, expected: &str, text: &str) -> io::Result<bool>;
}

#[derive(Default)]
pub struct SystemFiles;

impl FileSystemPort for SystemFiles {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        path.canonicalize()
    }

    fn project_files(&self, root: &Path) -> io::Result<Vec<ProjectFile>> {
        enumerate_project_files(root)
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

    fn write_text_if_unchanged(&self, path: &Path, expected: &str, text: &str) -> io::Result<bool> {
        if self.read_text(path, soyeht_protocol::limits::EDITOR_HISTORY_BYTES)? != expected {
            return Ok(false);
        }
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
            fs::File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result.map(|()| true)
    }
}

struct OpenDocument {
    file: EditorFile,
    syntax: Option<SyntaxDocument>,
    save_due_ms: Option<i64>,
    watch_due_ms: i64,
}

pub struct Service {
    files: Box<dyn FileSystemPort>,
    open: BTreeMap<String, OpenDocument>,
}

pub struct ResolvedPath {
    pub canonical: PathBuf,
    pub relative: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFile {
    pub canonical: PathBuf,
    pub relative: String,
    pub byte_count: u64,
    pub modified_ms: i64,
}

pub struct PreparedEdit {
    pub file: EditorFile,
    pub changed_lines: Vec<u32>,
    pub edits: Vec<TextEdit>,
}

const WATCH_INTERVAL_MS: i64 = 250;

impl Service {
    pub fn new(files: Box<dyn FileSystemPort>) -> Self {
        Self {
            files,
            open: BTreeMap::new(),
        }
    }

    pub fn restore(&mut self, file: EditorFile, now_ms: i64) {
        let save_due_ms = (file.draft_text.is_some() && file.conflict_text.is_none())
            .then_some(now_ms.saturating_add(autosave_ms()));
        self.open.insert(
            file.file_id.clone(),
            OpenDocument {
                file,
                syntax: None,
                save_due_ms,
                watch_due_ms: 0,
            },
        );
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
        contained_path(&root, target)
    }

    pub fn resolve_absolute(
        &self,
        workspace_root: &Path,
        requested: &Path,
    ) -> Result<ResolvedPath> {
        if !requested.is_absolute() {
            return Err(errors::invalid("Language result path must be absolute"));
        }
        let root = self
            .files
            .canonicalize(workspace_root)
            .map_err(errors::io)?;
        let target = self.files.canonicalize(requested).map_err(errors::io)?;
        contained_path(&root, target)
    }

    pub fn resolve_absolute_if_contained(
        &self,
        workspace_root: &Path,
        requested: &Path,
    ) -> Result<Option<ResolvedPath>> {
        if !requested.is_absolute() {
            return Err(errors::invalid("Agent file path must be absolute"));
        }
        let root = self
            .files
            .canonicalize(workspace_root)
            .map_err(errors::io)?;
        let target = match self.files.canonicalize(requested) {
            Ok(target) => target,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(errors::io(error)),
        };
        if target == root || !target.starts_with(&root) {
            return Ok(None);
        }
        contained_path(&root, target).map(Some)
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

    pub fn project_files(&self, workspace_root: &Path) -> Result<Vec<ProjectFile>> {
        let root = self
            .files
            .canonicalize(workspace_root)
            .map_err(errors::io)?;
        self.files.project_files(&root).map_err(errors::io)
    }

    pub fn project_text(&self, workspace_id: &str, file: &ProjectFile) -> Result<Option<String>> {
        if let Some(open) = self.open.values().find(|open| {
            open.file.workspace_id == workspace_id && open.file.relative_path == file.relative
        }) {
            return Ok(Some(
                open.file
                    .draft_text
                    .clone()
                    .unwrap_or_else(|| open.file.disk_text.clone()),
            ));
        }
        match self.files.read_text(
            &file.canonical,
            soyeht_protocol::limits::EDITOR_HISTORY_BYTES,
        ) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(errors::io(error)),
        }
    }

    pub fn project_disk_text(&self, file: &ProjectFile) -> Result<Option<String>> {
        match self.files.read_text(
            &file.canonical,
            soyeht_protocol::limits::EDITOR_HISTORY_BYTES,
        ) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(errors::io(error)),
        }
    }

    pub fn current_disk(&self, workspace_root: &Path, file: &EditorFile) -> Result<String> {
        let path = self.resolve(workspace_root, &file.relative_path)?;
        self.read_disk(&path.canonical)
    }

    pub fn save_disk(
        &self,
        workspace_root: &Path,
        file: &EditorFile,
        expected: &str,
        text: &str,
    ) -> Result<bool> {
        let path = self.resolve(workspace_root, &file.relative_path)?;
        self.files
            .write_text_if_unchanged(&path.canonical, expected, text)
            .map_err(errors::io)
    }

    pub fn insert(&mut self, file: EditorFile) {
        self.insert_with_syntax(file, None);
    }

    pub fn insert_parsed(&mut self, file: EditorFile, syntax: SyntaxDocument) {
        self.insert_with_syntax(file, Some(syntax));
    }

    fn insert_with_syntax(&mut self, file: EditorFile, syntax: Option<SyntaxDocument>) {
        self.open.insert(
            file.file_id.clone(),
            OpenDocument {
                file,
                syntax,
                save_due_ms: None,
                watch_due_ms: 0,
            },
        );
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

    pub fn is_open(&self, file_id: &str) -> bool {
        self.open.contains_key(file_id)
    }

    pub fn prepare_edit(
        &self,
        file_id: &str,
        base_revision: u64,
        edits: &[TextEdit],
        author: FileAuthor,
        now_ms: i64,
    ) -> Result<PreparedEdit> {
        let current = self.stored(file_id)?;
        if current.revision != base_revision {
            return Err(errors::stale_version("File revision changed"));
        }
        let mut file = current.clone();
        if edits.is_empty() {
            return Ok(PreparedEdit {
                file,
                changed_lines: Vec::new(),
                edits: Vec::new(),
            });
        }
        let text = current.draft_text.as_deref().unwrap_or(&current.disk_text);
        let mut buffer = TextBuffer::new(text);
        let applied = buffer.apply_all(edits)?;
        let next_text = buffer.text();
        if next_text.len() > soyeht_protocol::limits::EDITOR_HISTORY_BYTES {
            return Err(errors::invalid("Edited file exceeds the editor size limit"));
        }
        if let Some(disk) = file.conflict_text.take() {
            file.disk_text = disk;
        }
        file.revision = file
            .revision
            .checked_add(1)
            .ok_or_else(|| errors::internal("File revision exhausted"))?;
        file.draft_text = Some(next_text);
        file.draft_author = Some(author);
        file.updated_ms = now_ms;
        Ok(PreparedEdit {
            file,
            changed_lines: changed_lines(&applied),
            edits: edits.to_vec(),
        })
    }

    pub fn commit_edit(&mut self, prepared: PreparedEdit) {
        let Some(item) = self.open.get_mut(&prepared.file.file_id) else {
            return;
        };
        if !prepared.edits.is_empty() {
            let old_text = item
                .file
                .draft_text
                .as_deref()
                .unwrap_or(&item.file.disk_text);
            let language = item.file.language.clone();
            let syntax = item
                .syntax
                .take()
                .and_then(|mut document| {
                    (document.buffer().text() == old_text)
                        .then(|| document.apply_all(&prepared.edits).ok().map(|_| document))
                        .flatten()
                })
                .or_else(|| SyntaxDocument::new(&prepared.file.draft_text.clone()?, language).ok());
            item.syntax = syntax;
            item.save_due_ms = Some(prepared.file.updated_ms.saturating_add(autosave_ms()));
        }
        item.file = prepared.file;
    }

    pub fn replace(&mut self, file: EditorFile, now_ms: i64) {
        self.replace_with_syntax(file, now_ms, false);
    }

    pub fn replace_preserving_syntax(&mut self, file: EditorFile, now_ms: i64) {
        self.replace_with_syntax(file, now_ms, true);
    }

    fn replace_with_syntax(&mut self, file: EditorFile, now_ms: i64, preserve: bool) {
        if let Some(item) = self.open.get_mut(&file.file_id) {
            if !preserve {
                item.syntax = None;
            }
            item.save_due_ms = (file.draft_text.is_some() && file.conflict_text.is_none())
                .then_some(now_ms.saturating_add(autosave_ms()));
            item.watch_due_ms = now_ms.saturating_add(WATCH_INTERVAL_MS);
            item.file = file;
        }
    }

    pub fn due_saves(&self, now_ms: i64) -> Vec<String> {
        self.open
            .iter()
            .filter(|(_, item)| item.save_due_ms.is_some_and(|due| due <= now_ms))
            .map(|(file_id, _)| file_id.clone())
            .collect()
    }

    pub fn due_watches(&self, now_ms: i64) -> Vec<String> {
        self.open
            .iter()
            .filter(|(_, item)| item.file.watching && item.watch_due_ms <= now_ms)
            .map(|(file_id, _)| file_id.clone())
            .collect()
    }

    pub fn checked_watch(&mut self, file_id: &str, now_ms: i64) {
        if let Some(item) = self.open.get_mut(file_id) {
            item.watch_due_ms = now_ms.saturating_add(WATCH_INTERVAL_MS);
        }
    }

    pub fn defer_save(&mut self, file_id: &str, now_ms: i64) {
        if let Some(item) = self.open.get_mut(file_id) {
            item.save_due_ms = Some(now_ms.saturating_add(autosave_ms()));
        }
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
        self.open
            .retain(|_, item| item.file.pane_id.as_deref() != Some(pane_id));
    }
}

fn autosave_ms() -> i64 {
    i64::try_from(soyeht_protocol::limits::EDITOR_AUTOSAVE_MS).unwrap_or(i64::MAX)
}

fn contained_path(root: &Path, target: PathBuf) -> Result<ResolvedPath> {
    if target == root || !target.starts_with(root) {
        return Err(errors::invalid("File path escapes the workspace"));
    }
    let relative = target
        .strip_prefix(root)
        .map_err(|_| errors::invalid("File path escapes the workspace"))?
        .to_str()
        .ok_or_else(|| errors::invalid("File path must be UTF-8"))?
        .to_owned();
    Ok(ResolvedPath {
        canonical: target,
        relative,
    })
}

fn enumerate_project_files(root: &Path) -> io::Result<Vec<ProjectFile>> {
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

fn changed_lines(applied: &[soyeht_editor::AppliedEdit]) -> Vec<u32> {
    let mut lines = BTreeSet::new();
    for edit in applied {
        let end = edit.old_end_line.max(edit.new_end_line);
        for line in edit.start_line..=end {
            if lines.len() >= soyeht_protocol::limits::EDITOR_VIEWPORT_LINES {
                return lines.into_iter().collect();
            }
            if let Ok(line) = u32::try_from(line) {
                lines.insert(line);
            }
        }
    }
    lines.into_iter().collect()
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
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

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
        fn project_files(&self, _root: &Path) -> io::Result<Vec<ProjectFile>> {
            Ok(Vec::new())
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
        fn write_text_if_unchanged(
            &self,
            path: &Path,
            _expected: &str,
            text: &str,
        ) -> io::Result<bool> {
            self.writes
                .lock()
                .map_err(|_| io::Error::other("fixture lock"))?
                .push((path.into(), text.into()));
            Ok(true)
        }
    }

    struct SwappingFiles {
        root: PathBuf,
        target: PathBuf,
        outside: PathBuf,
        target_resolutions: AtomicUsize,
        wrote: Arc<AtomicBool>,
    }

    impl FileSystemPort for SwappingFiles {
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            if path == self.root {
                return Ok(self.root.clone());
            }
            if path == self.root.join("note.txt") {
                let call = self.target_resolutions.fetch_add(1, Ordering::SeqCst);
                return Ok(if call == 0 {
                    self.target.clone()
                } else {
                    self.outside.clone()
                });
            }
            Err(io::Error::new(io::ErrorKind::NotFound, "fixture path"))
        }

        fn project_files(&self, _root: &Path) -> io::Result<Vec<ProjectFile>> {
            Ok(Vec::new())
        }

        fn read_text(&self, _path: &Path, _limit: usize) -> io::Result<String> {
            Ok("before".into())
        }

        fn write_text_if_unchanged(
            &self,
            _path: &Path,
            _expected: &str,
            _text: &str,
        ) -> io::Result<bool> {
            self.wrote.store(true, Ordering::SeqCst);
            Ok(true)
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
        let absolute = service
            .resolve_absolute(&root, &root.join("src/main.rs"))
            .unwrap();
        assert_eq!(absolute.relative, "src/main.rs");
        assert!(service.resolve(&root, "/outside/secret.rs").is_err());
        assert!(service.resolve(&root, "../secret.rs").is_err());
        assert!(service.resolve(&root, "link.rs").is_err());
        assert!(
            service
                .resolve_absolute(&root, &PathBuf::from("/outside/secret.rs"))
                .is_err()
        );
        assert!(
            service
                .resolve_absolute(&root, &root.join("link.rs"))
                .is_err()
        );
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

    #[test]
    fn conditional_write_re_resolves_and_rejects_a_swapped_symlink_target() {
        let root = PathBuf::from("/repo");
        let wrote = Arc::new(AtomicBool::new(false));
        let service = Service::new(Box::new(SwappingFiles {
            root: root.clone(),
            target: root.join("note.txt"),
            outside: PathBuf::from("/outside/secret.txt"),
            target_resolutions: AtomicUsize::new(0),
            wrote: wrote.clone(),
        }));
        let file = EditorFile {
            file_id: "file-1".into(),
            pane_id: None,
            workspace_id: "workspace-1".into(),
            relative_path: "note.txt".into(),
            language: EditorLanguage::PlainText,
            revision: 0,
            disk_revision: 0,
            disk_text: "before".into(),
            draft_text: None,
            draft_author: None,
            conflict_text: None,
            watching: false,
            updated_ms: 0,
        };

        assert_eq!(service.current_disk(&root, &file).unwrap(), "before");
        assert!(service.save_disk(&root, &file, "before", "after").is_err());
        assert!(!wrote.load(Ordering::SeqCst));
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
        std::os::unix::fs::symlink(&outside, root.join("src/link.txt")).unwrap();

        let files = SystemFiles.project_files(&root).unwrap();
        let paths = files
            .into_iter()
            .map(|file| file.relative)
            .collect::<Vec<_>>();
        assert_eq!(paths, vec![".gitignore", "src/main.rs"]);

        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }
}

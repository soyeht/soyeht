use super::creation::{create_contained_directories, validate_relative_creation_path};
use super::schedule::{WATCH_INTERVAL_MS, save_backoff, watch_backoff};
use super::{
    project_files::{ProjectFile, save_temporary_name},
    project_reader::{ProjectReader, disk_text, list_project_files},
    text_refusal::TextRefusal,
};
use crate::{errors, errors::Result, streak::Streak};
use soyeht_editor::{
    AppliedStep, EditHistory, EditStep, HistoryDirection, Replay, SyntaxDocument, TextBuffer,
};
use soyeht_protocol::{EditorLanguage, EditorLine, FileAuthor, TextEdit, TextPosition};
use soyeht_store::editor::EditorFile;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// Shared with project searches that run off the Core lock (ADR 0079).
pub trait FileSystemPort: Send + Sync {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
    fn project_files(&self, root: &Path) -> io::Result<Vec<ProjectFile>>;
    fn read_text(&self, path: &Path, limit: usize) -> io::Result<String>;
    fn read_binary(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>>;
    fn write_text_if_unchanged(&self, path: &Path, expected: &str, text: &str) -> io::Result<bool>;
    fn creation_target_exists(&self, root: &Path, relative: &Path) -> io::Result<bool>;
    fn create_empty_text(&self, root: &Path, relative: &Path) -> io::Result<PathBuf>;
    fn create_binary_if_absent_or_matching(
        &self,
        root: &Path,
        relative: &Path,
        bytes: &[u8],
    ) -> io::Result<PathBuf>;
}

#[derive(Default)]
pub struct SystemFiles;

impl FileSystemPort for SystemFiles {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        soyeht_platform::fs::canonicalize(path)
    }

    fn project_files(&self, root: &Path) -> io::Result<Vec<ProjectFile>> {
        super::project_files::enumerate_project_files(root)
    }

    fn read_text(&self, path: &Path, limit: usize) -> io::Result<String> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file is not a regular file",
            ));
        }
        if metadata.len() > limit as u64 {
            return Err(TextRefusal::TooLarge.into());
        }
        let bytes = fs::read(path)?;
        if bytes.len() > limit {
            return Err(TextRefusal::TooLarge.into());
        }
        String::from_utf8(bytes).map_err(|_| TextRefusal::NotText.into())
    }

    fn read_binary(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        let metadata = fs::symlink_metadata(path)?;
        let limit = u64::try_from(limit)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "binary limit is invalid"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "asset is not a regular file within the writing image limit",
            ));
        }
        let bytes = fs::read(path)?;
        if bytes.is_empty()
            || u64::try_from(bytes.len())
                .ok()
                .is_none_or(|size| size > limit)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "asset is outside the writing image limit",
            ));
        }
        Ok(bytes)
    }

    fn write_text_if_unchanged(&self, path: &Path, expected: &str, text: &str) -> io::Result<bool> {
        if self.read_text(path, soyeht_protocol::limits::EDITOR_HISTORY_BYTES)? != expected {
            return Ok(false);
        }
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "file has no parent directory")
        })?;
        let temporary = parent.join(save_temporary_name());
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
            soyeht_platform::fs::sync_directory(parent)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary); // Best effort: the write error is returned.
        }
        result.map(|()| true)
    }

    fn creation_target_exists(&self, root: &Path, relative: &Path) -> io::Result<bool> {
        let root = soyeht_platform::fs::canonicalize(root)?;
        validate_relative_creation_path(relative)?;
        let target = root.join(relative);
        match fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "document target cannot be a symbolic link",
                    ));
                }
                let canonical = soyeht_platform::fs::canonicalize(&target)?;
                if !canonical.starts_with(&root) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "document target leaves the workspace",
                    ));
                }
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn create_empty_text(&self, root: &Path, relative: &Path) -> io::Result<PathBuf> {
        let root = soyeht_platform::fs::canonicalize(root)?;
        validate_relative_creation_path(relative)?;
        let parent = create_contained_directories(&root, relative)?;
        let filename = relative.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "document filename is missing")
        })?;
        let target = parent.join(filename);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(file) => file.sync_all()?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&target)?;
                if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "document target already exists",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
        soyeht_platform::fs::sync_directory(&parent)?;
        let canonical = soyeht_platform::fs::canonicalize(&target)?;
        if !canonical.starts_with(&root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "document target leaves the workspace",
            ));
        }
        Ok(canonical)
    }

    fn create_binary_if_absent_or_matching(
        &self,
        root: &Path,
        relative: &Path,
        bytes: &[u8],
    ) -> io::Result<PathBuf> {
        if bytes.is_empty() || bytes.len() > soyeht_protocol::limits::WRITING_IMAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "writing image is outside the byte budget",
            ));
        }
        let root = soyeht_platform::fs::canonicalize(root)?;
        validate_relative_creation_path(relative)?;
        let parent = create_contained_directories(&root, relative)?;
        let filename = relative.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "image filename is missing")
        })?;
        let target = parent.join(filename);
        crate::immutable_file::publish(&target, bytes)?;
        let canonical = soyeht_platform::fs::canonicalize(&target)?;
        if !canonical.starts_with(&root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "writing image leaves the workspace",
            ));
        }
        Ok(canonical)
    }
}

pub(super) struct OpenDocument {
    pub(super) file: EditorFile,
    syntax: Option<SyntaxDocument>,
    history: EditHistory,
    pub(super) save_due_ms: Option<i64>,
    pub(super) watch_due_ms: i64,
    pub(super) save_backoff: Streak,
    pub(super) watch_backoff: Streak,
}

pub struct Service {
    files: Arc<dyn FileSystemPort>,
    pub(super) open: BTreeMap<String, OpenDocument>,
}

pub struct ResolvedPath {
    pub canonical: PathBuf,
    pub relative: String,
}

pub struct PreparedEdit {
    pub file: EditorFile,
    pub changed_lines: Vec<u32>,
    /// Edit sets applied one after another; a plain write has exactly one.
    steps: Vec<Vec<TextEdit>>,
    history: HistoryChange,
    pub cursor: Option<TextPosition>,
}

enum HistoryChange {
    Record { base: u64, step: EditStep },
    Replay(HistoryDirection),
}

impl Service {
    pub fn new(files: Box<dyn FileSystemPort>) -> Self {
        Self {
            files: Arc::from(files),
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
                history: EditHistory::default(),
                save_due_ms,
                watch_due_ms: 0,
                save_backoff: save_backoff(),
                watch_backoff: watch_backoff(),
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
                match error
                    .get_ref()
                    .and_then(|inner| inner.downcast_ref::<TextRefusal>())
                {
                    Some(TextRefusal::NotText) => errors::file_not_text(error.to_string()),
                    Some(TextRefusal::TooLarge) => errors::file_too_large(error.to_string()),
                    None if error.kind() == io::ErrorKind::InvalidData => {
                        errors::invalid(error.to_string())
                    }
                    None => errors::io(error),
                }
            })
    }

    pub fn read_binary(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        self.files.read_binary(path, limit).map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidData {
                errors::invalid(error.to_string())
            } else {
                errors::io(error)
            }
        })
    }

    pub fn project_files(&self, workspace_root: &Path) -> Result<Vec<ProjectFile>> {
        list_project_files(&*self.files, workspace_root)
    }

    /// A reader for one workspace's files that a search uses off the lock.
    pub fn project_reader(&self, workspace_id: &str) -> ProjectReader {
        let open = self
            .open
            .values()
            .filter(|open| open.file.workspace_id == workspace_id)
            .map(|open| {
                let text = open
                    .file
                    .draft_text
                    .as_ref()
                    .unwrap_or(&open.file.disk_text);
                (open.file.relative_path.clone(), text.clone())
            })
            .collect();
        ProjectReader {
            files: Arc::clone(&self.files),
            open,
        }
    }

    pub fn creation_target_exists(&self, workspace_root: &Path, relative: &str) -> Result<bool> {
        self.files
            .creation_target_exists(workspace_root, Path::new(relative))
            .map_err(|error| {
                if matches!(
                    error.kind(),
                    io::ErrorKind::InvalidInput | io::ErrorKind::PermissionDenied
                ) {
                    errors::invalid(error.to_string())
                } else {
                    errors::io(error)
                }
            })
    }

    pub fn create_empty_text(&self, workspace_root: &Path, relative: &str) -> Result<PathBuf> {
        self.files
            .create_empty_text(workspace_root, Path::new(relative))
            .map_err(|error| {
                if matches!(
                    error.kind(),
                    io::ErrorKind::AlreadyExists
                        | io::ErrorKind::InvalidInput
                        | io::ErrorKind::PermissionDenied
                ) {
                    errors::invalid(error.to_string())
                } else {
                    errors::io(error)
                }
            })
    }

    pub fn create_binary_if_absent_or_matching(
        &self,
        workspace_root: &Path,
        relative: &str,
        bytes: &[u8],
    ) -> Result<PathBuf> {
        self.files
            .create_binary_if_absent_or_matching(workspace_root, Path::new(relative), bytes)
            .map_err(|error| {
                if matches!(
                    error.kind(),
                    io::ErrorKind::AlreadyExists
                        | io::ErrorKind::InvalidInput
                        | io::ErrorKind::PermissionDenied
                ) {
                    errors::invalid(error.to_string())
                } else {
                    errors::io(error)
                }
            })
    }

    pub fn project_disk_text(&self, file: &ProjectFile) -> Result<Option<String>> {
        disk_text(&*self.files, file)
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
                history: EditHistory::default(),
                save_due_ms: None,
                watch_due_ms: 0,
                save_backoff: save_backoff(),
                watch_backoff: watch_backoff(),
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
        self.prepare_recorded(
            file_id,
            base_revision,
            edits,
            (author, now_ms),
            EditStep::record,
        )
    }

    /// Prepares edits the engine built itself (find and replace) against the
    /// whole document as one undo transaction of their own.
    pub fn prepare_command_edit(
        &self,
        file_id: &str,
        base_revision: u64,
        edits: &[TextEdit],
        author: FileAuthor,
        now_ms: i64,
    ) -> Result<PreparedEdit> {
        let record = EditStep::record_command;
        self.prepare_recorded(file_id, base_revision, edits, (author, now_ms), record)
    }

    fn prepare_recorded(
        &self,
        file_id: &str,
        base_revision: u64,
        edits: &[TextEdit],
        (author, now_ms): (FileAuthor, i64),
        record: fn(&mut TextBuffer, &[TextEdit]) -> soyeht_editor::Result<AppliedStep>,
    ) -> Result<PreparedEdit> {
        let (item, mut buffer) = self.revision_buffer(file_id, base_revision)?;
        let recorded = record(&mut buffer, edits)?;
        let history = HistoryChange::Record {
            base: base_revision,
            step: recorded.step,
        };
        if edits.is_empty() {
            return Ok(PreparedEdit {
                file: item.file.clone(),
                changed_lines: Vec::new(),
                steps: Vec::new(),
                history,
                cursor: None,
            });
        }
        let replay = Replay {
            edits: vec![edits.to_vec()],
            applied: recorded.applied,
            cursor: recorded.cursor,
        };
        prepared_text(&item.file, &buffer, replay, history, author, now_ms)
    }

    /// Replays the newest undo or redo transaction, or returns `None` when the
    /// history is empty or no longer matches `base_revision`.
    pub fn prepare_replay(
        &self,
        file_id: &str,
        base_revision: u64,
        direction: HistoryDirection,
        author: FileAuthor,
        now_ms: i64,
    ) -> Result<Option<PreparedEdit>> {
        let (item, mut buffer) = self.revision_buffer(file_id, base_revision)?;
        let Some(replay) = item.history.replay(direction, base_revision, &mut buffer)? else {
            return Ok(None);
        };
        let history = HistoryChange::Replay(direction);
        prepared_text(&item.file, &buffer, replay, history, author, now_ms).map(Some)
    }

    fn revision_buffer(
        &self,
        file_id: &str,
        base_revision: u64,
    ) -> Result<(&OpenDocument, TextBuffer)> {
        let item = self
            .open
            .get(file_id)
            .ok_or_else(|| errors::not_found("Open file not found"))?;
        if item.file.revision != base_revision {
            return Err(errors::stale_version("File revision changed"));
        }
        let text = item
            .file
            .draft_text
            .as_deref()
            .unwrap_or(&item.file.disk_text);
        Ok((item, TextBuffer::new(text)))
    }

    pub fn commit_edit(&mut self, prepared: PreparedEdit) {
        let Some(item) = self.open.get_mut(&prepared.file.file_id) else {
            return;
        };
        match prepared.history {
            HistoryChange::Record { base, step } => {
                item.history
                    .record(base, prepared.file.revision, step, prepared.file.updated_ms)
            }
            HistoryChange::Replay(direction) => {
                item.history.commit(direction, prepared.file.revision)
            }
        }
        if !prepared.steps.is_empty() {
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
                        .then(|| {
                            prepared
                                .steps
                                .iter()
                                .try_for_each(|edits| document.apply_all(edits).map(|_| ()))
                                .ok()
                                .map(|()| document)
                        })
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

    fn open_item(&mut self, file_id: &str) -> Result<&mut OpenDocument> {
        self.open
            .get_mut(file_id)
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
        let item = self.open_item(file_id)?;
        let document = current_document(&mut item.syntax, text, language)?;
        soyeht_editor::project(document, from_line, line_count).map_err(Into::into)
    }

    /// The lines of the file's current buffer (its draft, else its disk text), read from the
    /// text the service already holds: a large file is not copied for each page.
    pub fn viewport_of_buffer(
        &mut self,
        file_id: &str,
        from_line: usize,
        line_count: usize,
    ) -> Result<Vec<EditorLine>> {
        let item = self.open_item(file_id)?;
        let text = item
            .file
            .draft_text
            .as_deref()
            .unwrap_or(&item.file.disk_text);
        let document = current_document(&mut item.syntax, text, item.file.language.clone())?;
        soyeht_editor::project(document, from_line, line_count).map_err(Into::into)
    }

    pub fn remove_pane(&mut self, pane_id: &str) {
        self.open
            .retain(|_, item| item.file.pane_id.as_deref() != Some(pane_id));
    }
}

/// The syntax document of `text`: the one held when it holds exactly this text, else a new one.
fn current_document<'a>(
    syntax: &'a mut Option<SyntaxDocument>,
    text: &str,
    language: EditorLanguage,
) -> Result<&'a SyntaxDocument> {
    if syntax
        .as_ref()
        .is_none_or(|document| !document.buffer().matches(text))
    {
        *syntax = Some(SyntaxDocument::new(text, language)?);
    }
    syntax
        .as_ref()
        .ok_or_else(|| errors::internal("Editor document was not initialized"))
}

pub(super) fn autosave_ms() -> i64 {
    i64::try_from(soyeht_protocol::limits::EDITOR_AUTOSAVE_MS).unwrap_or(i64::MAX)
}

fn contained_path(root: &Path, target: PathBuf) -> Result<ResolvedPath> {
    if target == root || !target.starts_with(root) {
        return Err(errors::invalid("File path escapes the workspace"));
    }
    let relative = target
        .strip_prefix(root)
        .map_err(|_| errors::invalid("File path escapes the workspace"))
        .and_then(super::checked_wire_path)?;
    Ok(ResolvedPath {
        canonical: target,
        relative,
    })
}

fn prepared_text(
    current: &EditorFile,
    buffer: &TextBuffer,
    replay: Replay,
    history: HistoryChange,
    author: FileAuthor,
    now_ms: i64,
) -> Result<PreparedEdit> {
    let next_text = buffer.text();
    if next_text.len() > soyeht_protocol::limits::EDITOR_HISTORY_BYTES {
        return Err(errors::invalid("Edited file exceeds the editor size limit"));
    }
    let mut file = current.clone();
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
        changed_lines: changed_lines(&replay.applied),
        steps: replay.edits,
        history,
        cursor: replay.cursor,
    })
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

#[cfg(test)]
mod tests;

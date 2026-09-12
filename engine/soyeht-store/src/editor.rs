//! Named persistence operations for editor ownership, drafts, and file history.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, params};
use soyeht_protocol::{EditorLanguage, FileAuthor, FileDocument, FileVersion};

#[derive(Clone, Debug, PartialEq)]
pub struct EditorFile {
    pub file_id: String,
    pub pane_id: String,
    pub workspace_id: String,
    pub relative_path: String,
    pub language: EditorLanguage,
    pub revision: u64,
    pub disk_revision: u64,
    pub disk_text: String,
    pub draft_text: Option<String>,
    pub draft_author: Option<FileAuthor>,
    pub conflict_text: Option<String>,
    pub watching: bool,
    pub updated_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredFileVersion {
    pub version: FileVersion,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileHistoryPage {
    pub items: Vec<FileVersion>,
    pub next_cursor: Option<String>,
}

struct RawEditorFile {
    file_id: String,
    pane_id: String,
    workspace_id: String,
    relative_path: String,
    language: String,
    revision: i64,
    disk_revision: i64,
    disk_text: String,
    draft_text: Option<String>,
    draft_author: Option<String>,
    conflict_text: Option<String>,
    watching: bool,
    updated_ms: i64,
}

impl Store {
    pub fn editor_file(&self, file_id: &str) -> Result<Option<EditorFile>> {
        let raw = self
            .connection
            .query_row(
                "SELECT file_id,pane_id,workspace_id,relative_path,language,revision,disk_revision,disk_text,draft_text,draft_author,conflict_text,watching,updated_ms FROM editor_files WHERE file_id=?1",
                [file_id],
                decode_file,
            )
            .optional()?;
        raw.map(validate_file).transpose()
    }

    pub fn editor_file_for_pane(&self, pane_id: &str) -> Result<Option<EditorFile>> {
        let raw = self
            .connection
            .query_row(
                "SELECT file_id,pane_id,workspace_id,relative_path,language,revision,disk_revision,disk_text,draft_text,draft_author,conflict_text,watching,updated_ms FROM editor_files WHERE pane_id=?1",
                [pane_id],
                decode_file,
            )
            .optional()?;
        raw.map(validate_file).transpose()
    }

    pub fn editor_files(&self) -> Result<Vec<EditorFile>> {
        let mut statement = self.connection.prepare(
            "SELECT e.file_id,e.pane_id,e.workspace_id,e.relative_path,e.language,e.revision,e.disk_revision,e.disk_text,e.draft_text,e.draft_author,e.conflict_text,e.watching,e.updated_ms FROM editor_files e JOIN panes p ON p.id=e.pane_id WHERE p.closed=0 ORDER BY e.workspace_id,e.relative_path",
        )?;
        let rows = statement.query_map([], decode_file)?;
        rows.map(|row| validate_file(row?)).collect()
    }

    pub fn file_version(
        &self,
        file_id: &str,
        version_id: &str,
    ) -> Result<Option<StoredFileVersion>> {
        let raw = self
            .connection
            .query_row(
                "SELECT revision,created_at,author,content,byte_count FROM editor_file_versions WHERE file_id=?1 AND version_id=?2",
                params![file_id, version_id],
                |row| decode_version(row, file_id, version_id),
            )
            .optional()?;
        raw.map(validate_version).transpose()
    }

    pub fn file_history(
        &self,
        file_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<FileHistoryPage> {
        if limit == 0 || limit > soyeht_protocol::limits::EDITOR_HISTORY_VERSIONS {
            return Err(Error::InvalidFileHistoryLimit);
        }
        let before = cursor
            .map(|value| history_ordinal(&self.connection, file_id, value))
            .transpose()?
            .unwrap_or(i64::MAX);
        let query_limit =
            i64::try_from(limit.saturating_add(1)).map_err(|_| Error::InvalidFileHistoryLimit)?;
        let mut statement = self.connection.prepare(
            "SELECT version_id,revision,created_at,author,byte_count FROM editor_file_versions WHERE file_id=?1 AND ordinal<?2 ORDER BY ordinal DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(params![file_id, before, query_limit], |row| {
            decode_version_metadata(row, file_id)
        })?;
        let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        let has_more = items.len() > limit;
        items.truncate(limit);
        let next_cursor = has_more
            .then(|| items.last().map(|item| item.version_id.clone()))
            .flatten();
        Ok(FileHistoryPage { items, next_cursor })
    }
}

pub(crate) fn snapshot_files(transaction: &rusqlite::Transaction<'_>) -> Result<Vec<FileDocument>> {
    let mut statement = transaction.prepare(
        "SELECT e.file_id,e.pane_id,e.workspace_id,e.relative_path,e.language,e.revision,e.disk_revision,e.disk_text,e.draft_text,e.draft_author,e.conflict_text,e.watching,e.updated_ms FROM editor_files e JOIN panes p ON p.id=e.pane_id WHERE p.closed=0 ORDER BY e.workspace_id,e.relative_path",
    )?;
    let rows = statement.query_map([], decode_file)?;
    rows.map(|row| validate_file(row?).map(document)).collect()
}

pub fn document(file: EditorFile) -> FileDocument {
    let text = file.draft_text.as_deref().unwrap_or(&file.disk_text);
    FileDocument {
        file_id: file.file_id,
        pane_id: file.pane_id,
        workspace_id: file.workspace_id,
        path: file.relative_path,
        language: file.language,
        revision: file.revision.to_string(),
        disk_revision: file.disk_revision.to_string(),
        line_count: text.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1,
        byte_count: text.len().to_string(),
        dirty: file.draft_text.is_some(),
        conflicted: file.conflict_text.is_some(),
        watching: file.watching,
    }
}

impl Change<'_> {
    pub fn save_editor_file(&self, file: &EditorFile) -> Result<()> {
        validate_file_record(file)?;
        self.transaction.execute(
            "INSERT INTO editor_files(file_id,pane_id,workspace_id,relative_path,language,revision,disk_revision,disk_text,draft_text,draft_author,conflict_text,watching,updated_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) ON CONFLICT(file_id) DO UPDATE SET pane_id=excluded.pane_id,workspace_id=excluded.workspace_id,relative_path=excluded.relative_path,language=excluded.language,revision=excluded.revision,disk_revision=excluded.disk_revision,disk_text=excluded.disk_text,draft_text=excluded.draft_text,draft_author=excluded.draft_author,conflict_text=excluded.conflict_text,watching=excluded.watching,updated_ms=excluded.updated_ms",
            params![
                file.file_id,
                file.pane_id,
                file.workspace_id,
                file.relative_path,
                language_name(&file.language),
                to_i64(file.revision)?,
                to_i64(file.disk_revision)?,
                file.disk_text,
                file.draft_text,
                file.draft_author.as_ref().map(serde_json::to_string).transpose()?,
                file.conflict_text,
                file.watching,
                file.updated_ms,
            ],
        )?;
        Ok(())
    }

    pub fn record_file_version(&self, version: &StoredFileVersion) -> Result<()> {
        validate_version(version.clone())?;
        self.transaction.execute(
            "INSERT INTO editor_file_versions(version_id,file_id,revision,created_at,author,content,byte_count) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                version.version.version_id,
                version.version.file_id,
                decimal_to_i64(&version.version.revision)?,
                version.version.created_at,
                serde_json::to_string(&version.version.author)?,
                version.content,
                to_i64(version.content.len() as u64)?,
            ],
        )?;
        prune_history(self, &version.version.file_id)
    }

    pub fn clear_file_history(&self, file_id: &str) -> Result<()> {
        self.transaction.execute(
            "DELETE FROM editor_file_versions WHERE file_id=?1",
            [file_id],
        )?;
        Ok(())
    }
}

fn decode_file(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawEditorFile> {
    Ok(RawEditorFile {
        file_id: row.get(0)?,
        pane_id: row.get(1)?,
        workspace_id: row.get(2)?,
        relative_path: row.get(3)?,
        language: row.get(4)?,
        revision: row.get(5)?,
        disk_revision: row.get(6)?,
        disk_text: row.get(7)?,
        draft_text: row.get(8)?,
        draft_author: row.get(9)?,
        conflict_text: row.get(10)?,
        watching: row.get(11)?,
        updated_ms: row.get(12)?,
    })
}

fn validate_file(raw: RawEditorFile) -> Result<EditorFile> {
    let file = EditorFile {
        file_id: raw.file_id,
        pane_id: raw.pane_id,
        workspace_id: raw.workspace_id,
        relative_path: raw.relative_path,
        language: language_from_name(&raw.language).ok_or(Error::InvalidEditorFile)?,
        revision: u64::try_from(raw.revision).map_err(|_| Error::InvalidEditorFile)?,
        disk_revision: u64::try_from(raw.disk_revision).map_err(|_| Error::InvalidEditorFile)?,
        disk_text: raw.disk_text,
        draft_text: raw.draft_text,
        draft_author: raw
            .draft_author
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        conflict_text: raw.conflict_text,
        watching: raw.watching,
        updated_ms: raw.updated_ms,
    };
    validate_file_record(&file)?;
    Ok(file)
}

fn validate_file_record(file: &EditorFile) -> Result<()> {
    let identifiers = [&file.file_id, &file.pane_id, &file.workspace_id];
    let draft_matches = file.draft_text.is_some() == file.draft_author.is_some();
    if identifiers
        .iter()
        .any(|value| value.is_empty() || value.len() > 128)
        || file.relative_path.is_empty()
        || file.relative_path.len() > 4096
        || file.relative_path.starts_with('/')
        || file.updated_ms < 0
        || !draft_matches
        || (file.conflict_text.is_some() && file.draft_text.is_none())
    {
        return Err(Error::InvalidEditorFile);
    }
    Ok(())
}

fn validate_version(version: StoredFileVersion) -> Result<StoredFileVersion> {
    let metadata = &version.version;
    let expected = decimal_to_i64(&metadata.byte_count)?;
    if metadata.version_id.is_empty()
        || metadata.version_id.len() > 128
        || metadata.file_id.is_empty()
        || metadata.file_id.len() > 128
        || metadata.created_at.is_empty()
        || metadata.created_at.len() > 128
        || expected != to_i64(version.content.len() as u64)?
    {
        return Err(Error::InvalidEditorFile);
    }
    Ok(version)
}

fn prune_history(change: &Change<'_>, file_id: &str) -> Result<()> {
    let mut statement = change.transaction.prepare(
        "SELECT ordinal,byte_count FROM editor_file_versions WHERE file_id=?1 ORDER BY ordinal DESC",
    )?;
    let rows = statement.query_map([file_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut retained_bytes = 0_u64;
    let mut delete = Vec::new();
    for (index, row) in rows.enumerate() {
        let (ordinal, bytes) = row?;
        let bytes = u64::try_from(bytes).map_err(|_| Error::InvalidEditorFile)?;
        retained_bytes = retained_bytes.saturating_add(bytes);
        if index >= soyeht_protocol::limits::EDITOR_HISTORY_VERSIONS
            || retained_bytes > soyeht_protocol::limits::EDITOR_HISTORY_BYTES as u64
        {
            delete.push(ordinal);
        }
    }
    drop(statement);
    for ordinal in delete {
        change.transaction.execute(
            "DELETE FROM editor_file_versions WHERE ordinal=?1",
            [ordinal],
        )?;
    }
    Ok(())
}

fn history_ordinal(connection: &rusqlite::Connection, file_id: &str, cursor: &str) -> Result<i64> {
    connection
        .query_row(
            "SELECT ordinal FROM editor_file_versions WHERE file_id=?1 AND version_id=?2",
            params![file_id, cursor],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(Error::FileVersionNotFound)
}

fn decode_version(
    row: &rusqlite::Row<'_>,
    file_id: &str,
    version_id: &str,
) -> rusqlite::Result<StoredFileVersion> {
    let author: String = row.get(2)?;
    Ok(StoredFileVersion {
        version: FileVersion {
            version_id: version_id.to_owned(),
            file_id: file_id.to_owned(),
            revision: row.get::<_, i64>(0)?.to_string(),
            created_at: row.get(1)?,
            author: serde_json::from_str(&author).map_err(json_error)?,
            byte_count: row.get::<_, i64>(4)?.to_string(),
        },
        content: row.get(3)?,
    })
}

fn decode_version_metadata(
    row: &rusqlite::Row<'_>,
    file_id: &str,
) -> rusqlite::Result<FileVersion> {
    let author: String = row.get(3)?;
    Ok(FileVersion {
        version_id: row.get(0)?,
        file_id: file_id.to_owned(),
        revision: row.get::<_, i64>(1)?.to_string(),
        created_at: row.get(2)?,
        author: serde_json::from_str(&author).map_err(json_error)?,
        byte_count: row.get::<_, i64>(4)?.to_string(),
    })
}

fn json_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn to_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::InvalidEditorFile)
}

fn decimal_to_i64(value: &str) -> Result<i64> {
    value.parse().map_err(|_| Error::InvalidEditorFile)
}

fn language_name(language: &EditorLanguage) -> &'static str {
    match language {
        EditorLanguage::Rust => "rust",
        EditorLanguage::Swift => "swift",
        EditorLanguage::Typescript => "typescript",
        EditorLanguage::Python => "python",
        EditorLanguage::Json => "json",
        EditorLanguage::Markdown => "markdown",
        EditorLanguage::Shell => "shell",
        EditorLanguage::PlainText => "plain_text",
    }
}

fn language_from_name(value: &str) -> Option<EditorLanguage> {
    match value {
        "rust" => Some(EditorLanguage::Rust),
        "swift" => Some(EditorLanguage::Swift),
        "typescript" => Some(EditorLanguage::Typescript),
        "python" => Some(EditorLanguage::Python),
        "json" => Some(EditorLanguage::Json),
        "markdown" => Some(EditorLanguage::Markdown),
        "shell" => Some(EditorLanguage::Shell),
        "plain_text" => Some(EditorLanguage::PlainText),
        _ => None,
    }
}

//! Named persistence operations for editor ownership, drafts, and file history.

use crate::database::{Change, Error, Result, Store};
use rusqlite::{OptionalExtension, params};
use soyeht_protocol::{
    EditorLanguage, FileAuthor, FileDocument, FileSearchMatch, FileVersion, Role,
};

#[derive(Clone, Debug, PartialEq)]
pub struct EditorFile {
    pub file_id: String,
    pub pane_id: Option<String>,
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

#[derive(Clone, Debug, PartialEq)]
pub struct EditorAgentReview {
    pub file_id: String,
    pub revision: u64,
    pub pane_id: String,
    pub author: FileAuthor,
    pub baseline_text: String,
    pub updated_ms: i64,
}

struct RawEditorAgentReview {
    file_id: String,
    revision: i64,
    pane_id: String,
    author: String,
    baseline_text: String,
    updated_ms: i64,
}

struct RawEditorFile {
    file_id: String,
    pane_id: Option<String>,
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

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectReplacement {
    pub operation_id: String,
    pub scope: String,
    pub idempotency_key: String,
    pub fingerprint: String,
    pub workspace_id: String,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub author: FileAuthor,
    pub matches: Vec<FileSearchMatch>,
    pub total_replaced: u32,
    pub created_ms: i64,
    pub items: Vec<ProjectReplacementItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectReplacementItem {
    pub ordinal: u32,
    pub file_id: String,
    pub relative_path: String,
    pub before_hash: String,
    pub after_hash: String,
    pub replaced: u32,
    pub applied: bool,
}

impl Store {
    pub fn editor_agent_review(&self, file_id: &str) -> Result<Option<EditorAgentReview>> {
        let raw = self
            .connection
            .query_row(
                "SELECT file_id,revision,pane_id,author,baseline_text,updated_ms FROM editor_agent_reviews WHERE file_id=?1",
                [file_id],
                decode_agent_review,
            )
            .optional()?;
        raw.map(validate_agent_review).transpose()
    }

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

    pub fn editor_file_for_path(
        &self,
        workspace_id: &str,
        relative_path: &str,
    ) -> Result<Option<EditorFile>> {
        let raw = self
            .connection
            .query_row(
                "SELECT file_id,pane_id,workspace_id,relative_path,language,revision,disk_revision,disk_text,draft_text,draft_author,conflict_text,watching,updated_ms FROM editor_files WHERE workspace_id=?1 AND relative_path=?2",
                params![workspace_id, relative_path],
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

    pub fn file_replacement(
        &self,
        scope: &str,
        idempotency_key: &str,
    ) -> Result<Option<ProjectReplacement>> {
        replacement_by_key(&self.connection, scope, idempotency_key)
    }

    pub fn active_file_replacements(&self) -> Result<Vec<ProjectReplacement>> {
        let mut statement = self.connection.prepare(
            "SELECT operation_id FROM editor_replace_operations ORDER BY created_ms,operation_id",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| replacement_by_id(&self.connection, &id))
            .collect()
    }

    pub fn begin_file_replacement(
        &mut self,
        replacement: &ProjectReplacement,
        files: &[EditorFile],
    ) -> Result<ProjectReplacement> {
        validate_replacement(replacement, files)?;
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(previous) = replacement_by_key(
            &transaction,
            &replacement.scope,
            &replacement.idempotency_key,
        )? {
            if previous.fingerprint != replacement.fingerprint {
                return Err(Error::IdempotencyConflict);
            }
            return Ok(previous);
        }
        let busy = transaction
            .query_row(
                "SELECT 1 FROM editor_replace_operations WHERE workspace_id=?1",
                [&replacement.workspace_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if busy {
            return Err(Error::FileReplacementBusy);
        }
        let change = Change { transaction };
        for file in files {
            change.stage_replacement_file(file)?;
        }
        change.insert_file_replacement(replacement)?;
        change.transaction.commit()?;
        Ok(replacement.clone())
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
    rows.map(|row| validate_file(row?).and_then(document))
        .collect()
}

pub fn document(file: EditorFile) -> Result<FileDocument> {
    let text = file.draft_text.as_deref().unwrap_or(&file.disk_text);
    let pane_id = file.pane_id.ok_or(Error::InvalidEditorFile)?;
    Ok(FileDocument {
        file_id: file.file_id,
        pane_id,
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
    })
}

impl Change<'_> {
    pub fn save_editor_agent_review(&self, review: &EditorAgentReview) -> Result<()> {
        validate_agent_review_record(review)?;
        self.transaction.execute(
            "INSERT INTO editor_agent_reviews(file_id,revision,pane_id,author,baseline_text,updated_ms) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(file_id) DO UPDATE SET revision=excluded.revision,pane_id=excluded.pane_id,author=excluded.author,baseline_text=excluded.baseline_text,updated_ms=excluded.updated_ms",
            params![
                review.file_id,
                to_i64(review.revision)?,
                review.pane_id,
                serde_json::to_string(&review.author)?,
                review.baseline_text,
                review.updated_ms,
            ],
        )?;
        Ok(())
    }

    pub fn clear_editor_agent_review(&self, file_id: &str) -> Result<bool> {
        let removed = self.transaction.execute(
            "DELETE FROM editor_agent_reviews WHERE file_id=?1",
            [file_id],
        )?;
        Ok(removed == 1)
    }

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

    pub fn mark_file_replacement_applied(
        &self,
        operation_id: &str,
        ordinal: u32,
        file: &EditorFile,
        version: &StoredFileVersion,
    ) -> Result<bool> {
        validate_file_record(file)?;
        validate_version(version.clone())?;
        let row = self
            .transaction
            .query_row(
                "SELECT file_id,applied FROM editor_replace_items WHERE operation_id=?1 AND ordinal=?2",
                params![operation_id, i64::from(ordinal)],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
            )
            .optional()?
            .ok_or(Error::InvalidFileReplacement)?;
        if row.0 != file.file_id || version.version.file_id != file.file_id {
            return Err(Error::InvalidFileReplacement);
        }
        if row.1 {
            return Ok(false);
        }
        self.save_editor_file(file)?;
        self.record_file_version(version)?;
        let changed = self.transaction.execute(
            "UPDATE editor_replace_items SET applied=1 WHERE operation_id=?1 AND ordinal=?2 AND applied=0",
            params![operation_id, i64::from(ordinal)],
        )?;
        Ok(changed == 1)
    }

    pub fn complete_file_replacement(&self, operation_id: &str) -> Result<()> {
        let pending = self.transaction.query_row(
            "SELECT count(*) FROM editor_replace_items WHERE operation_id=?1 AND applied=0",
            [operation_id],
            |row| row.get::<_, u32>(0),
        )?;
        if pending != 0 {
            return Err(Error::FileReplacementIncomplete);
        }
        let removed = self.transaction.execute(
            "DELETE FROM editor_replace_operations WHERE operation_id=?1",
            [operation_id],
        )?;
        if removed != 1 {
            return Err(Error::InvalidFileReplacement);
        }
        Ok(())
    }

    fn stage_replacement_file(&self, file: &EditorFile) -> Result<()> {
        let previous = self
            .transaction
            .query_row(
                "SELECT file_id,pane_id,workspace_id,relative_path,language,revision,disk_revision,disk_text,draft_text,draft_author,conflict_text,watching,updated_ms FROM editor_files WHERE workspace_id=?1 AND relative_path=?2",
                params![file.workspace_id, file.relative_path],
                decode_file,
            )
            .optional()?
            .map(validate_file)
            .transpose()?;
        if let Some(previous) = previous {
            if previous != *file {
                return Err(Error::InvalidFileReplacement);
            }
        } else {
            self.save_editor_file(file)?;
        }
        Ok(())
    }

    fn insert_file_replacement(&self, replacement: &ProjectReplacement) -> Result<()> {
        self.transaction.execute(
            "INSERT INTO editor_replace_operations(operation_id,scope,idempotency_key,fingerprint,workspace_id,query,replacement,case_sensitive,author,matches,total_replaced,created_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                replacement.operation_id,
                replacement.scope,
                replacement.idempotency_key,
                replacement.fingerprint,
                replacement.workspace_id,
                replacement.query,
                replacement.replacement,
                replacement.case_sensitive,
                serde_json::to_string(&replacement.author)?,
                serde_json::to_string(&replacement.matches)?,
                replacement.total_replaced,
                replacement.created_ms,
            ],
        )?;
        for item in &replacement.items {
            self.transaction.execute(
                "INSERT INTO editor_replace_items(operation_id,ordinal,file_id,relative_path,before_hash,after_hash,replaced,applied) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    replacement.operation_id,
                    item.ordinal,
                    item.file_id,
                    item.relative_path,
                    item.before_hash,
                    item.after_hash,
                    item.replaced,
                    item.applied,
                ],
            )?;
        }
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

fn decode_agent_review(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawEditorAgentReview> {
    Ok(RawEditorAgentReview {
        file_id: row.get(0)?,
        revision: row.get(1)?,
        pane_id: row.get(2)?,
        author: row.get(3)?,
        baseline_text: row.get(4)?,
        updated_ms: row.get(5)?,
    })
}

fn validate_agent_review(raw: RawEditorAgentReview) -> Result<EditorAgentReview> {
    let review = EditorAgentReview {
        file_id: raw.file_id,
        revision: u64::try_from(raw.revision).map_err(|_| Error::InvalidEditorAgentReview)?,
        pane_id: raw.pane_id,
        author: serde_json::from_str(&raw.author)?,
        baseline_text: raw.baseline_text,
        updated_ms: raw.updated_ms,
    };
    validate_agent_review_record(&review)?;
    Ok(review)
}

fn validate_agent_review_record(review: &EditorAgentReview) -> Result<()> {
    let actor = &review.author.actor;
    let valid_role = matches!(actor.role, Role::Agent | Role::Chief);
    if review.file_id.is_empty()
        || review.file_id.len() > 128
        || review.pane_id.is_empty()
        || review.pane_id.len() > 128
        || review.author.name.is_empty()
        || review.author.name.len() > 128
        || actor.client.is_empty()
        || actor.client.len() > 128
        || actor.pane_id.as_deref() != Some(review.pane_id.as_str())
        || !valid_role
        || review.baseline_text.len() > soyeht_protocol::limits::EDITOR_HISTORY_BYTES
        || review.updated_ms < 0
    {
        return Err(Error::InvalidEditorAgentReview);
    }
    Ok(())
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
    let identifiers = [&file.file_id, &file.workspace_id];
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
        || (file.pane_id.is_none()
            && (file.draft_text.is_some() || file.conflict_text.is_some() || file.watching))
        || file
            .pane_id
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 128)
    {
        return Err(Error::InvalidEditorFile);
    }
    Ok(())
}

fn replacement_by_key(
    connection: &rusqlite::Connection,
    scope: &str,
    idempotency_key: &str,
) -> Result<Option<ProjectReplacement>> {
    let id = connection
        .query_row(
            "SELECT operation_id FROM editor_replace_operations WHERE scope=?1 AND idempotency_key=?2",
            params![scope, idempotency_key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    id.map(|id| replacement_by_id(connection, &id)).transpose()
}

fn replacement_by_id(
    connection: &rusqlite::Connection,
    operation_id: &str,
) -> Result<ProjectReplacement> {
    let mut replacement = connection.query_row(
        "SELECT scope,idempotency_key,fingerprint,workspace_id,query,replacement,case_sensitive,author,matches,total_replaced,created_ms FROM editor_replace_operations WHERE operation_id=?1",
        [operation_id],
        |row| {
            let author: String = row.get(7)?;
            let matches: String = row.get(8)?;
            Ok(ProjectReplacement {
                operation_id: operation_id.to_owned(),
                scope: row.get(0)?,
                idempotency_key: row.get(1)?,
                fingerprint: row.get(2)?,
                workspace_id: row.get(3)?,
                query: row.get(4)?,
                replacement: row.get(5)?,
                case_sensitive: row.get(6)?,
                author: serde_json::from_str(&author).map_err(json_error)?,
                matches: serde_json::from_str(&matches).map_err(json_error)?,
                total_replaced: row.get(9)?,
                created_ms: row.get(10)?,
                items: Vec::new(),
            })
        },
    )?;
    let mut statement = connection.prepare(
        "SELECT ordinal,file_id,relative_path,before_hash,after_hash,replaced,applied FROM editor_replace_items WHERE operation_id=?1 ORDER BY ordinal",
    )?;
    replacement.items = statement
        .query_map([operation_id], |row| {
            Ok(ProjectReplacementItem {
                ordinal: row.get(0)?,
                file_id: row.get(1)?,
                relative_path: row.get(2)?,
                before_hash: row.get(3)?,
                after_hash: row.get(4)?,
                replaced: row.get(5)?,
                applied: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    validate_replacement(&replacement, &[])?;
    Ok(replacement)
}

fn validate_replacement(replacement: &ProjectReplacement, files: &[EditorFile]) -> Result<()> {
    let ids = [
        &replacement.operation_id,
        &replacement.scope,
        &replacement.idempotency_key,
        &replacement.workspace_id,
    ];
    let items_match = files.is_empty()
        || (files.len() == replacement.items.len()
            && files.iter().zip(&replacement.items).all(|(file, item)| {
                file.file_id == item.file_id
                    && file.workspace_id == replacement.workspace_id
                    && file.relative_path == item.relative_path
            }));
    let ordinals_match = replacement
        .items
        .iter()
        .enumerate()
        .all(|(index, item)| usize::try_from(item.ordinal).ok() == Some(index));
    if ids
        .iter()
        .any(|value| value.is_empty() || value.len() > 128)
        || replacement.fingerprint.is_empty()
        || replacement.fingerprint.len() > 256
        || replacement.query.is_empty()
        || replacement.query.len() > 4096
        || replacement.replacement.len() > 262_144
        || replacement.created_ms < 0
        || replacement.items.is_empty()
        || replacement.items.len() > soyeht_protocol::limits::EDITOR_SEARCH_RESULTS
        || replacement.matches.len() > soyeht_protocol::limits::EDITOR_SEARCH_RESULTS
        || replacement.total_replaced == 0
        || replacement.total_replaced as usize > soyeht_protocol::limits::EDITOR_SEARCH_RESULTS
        || replacement.matches.len() != replacement.total_replaced as usize
        || replacement
            .items
            .iter()
            .map(|item| item.replaced)
            .try_fold(0_u32, u32::checked_add)
            != Some(replacement.total_replaced)
        || replacement.items.iter().any(|item| {
            item.file_id.is_empty()
                || item.file_id.len() > 128
                || item.relative_path.is_empty()
                || item.relative_path.len() > 4096
                || item.relative_path.starts_with('/')
                || !valid_hash(&item.before_hash)
                || !valid_hash(&item.after_hash)
                || item.before_hash == item.after_hash
                || item.replaced == 0
                || item.replaced as usize > soyeht_protocol::limits::EDITOR_SEARCH_RESULTS
        })
        || !items_match
        || !ordinals_match
    {
        return Err(Error::InvalidFileReplacement);
    }
    for file in files {
        validate_file_record(file)?;
        if file.draft_text.is_some() || file.conflict_text.is_some() {
            return Err(Error::InvalidFileReplacement);
        }
    }
    Ok(())
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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

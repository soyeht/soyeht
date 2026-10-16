use super::{Effect, Failure, service::PreparedEdit};
use crate::{
    core::Core,
    errors::{self, Result},
    model,
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_platform::log::Line;
use soyeht_protocol::{
    Actor, FileAuthor, FileHistoryParams, FileHistoryResult, FileRevertParams, FileRevertResult,
    FileWatchParams, FileWatchResult, FileWriteParams, FileWriteResult, Role, event, method,
};
use soyeht_store::editor::{EditorFile, StoredFileVersion, document};
use uuid::Uuid;

impl Core {
    pub fn file_write(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: FileWriteParams = parameters(params, "write")?;
        let author = self.file_author(connection)?;
        if request.author != author {
            return Err(errors::unauthorized(
                "File author does not match the connection",
            ));
        }
        let base = revision(&request.base_revision)?;
        let prepared = self.editor.prepare_edit(
            &request.file_id,
            base,
            &request.edits,
            author,
            self.now_ms(),
        )?;
        let result = serde_json::to_value(FileWriteResult {
            file: document(prepared.file.clone()).map_err(errors::storage)?,
            changed_lines: prepared.changed_lines.clone(),
        })
        .map_err(|_| errors::internal("File write result serialization failed"))?;
        self.commit_prepared_edit(connection, method::FILE_WRITE, params, prepared, result)
    }

    /// Persists a prepared buffer change, then applies it to the open document
    /// unless the request is an idempotent repeat.
    pub(super) fn commit_prepared_edit(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
        prepared: PreparedEdit,
        result: Value,
    ) -> Result<Value> {
        let file = prepared.file.clone();
        let mutation = self.mutate(connection, method, params, |change| {
            change.save_editor_file(&file)?;
            Ok((result, Vec::new()))
        })?;
        if !mutation.repeated {
            self.editor.commit_edit(prepared);
            self.track_language_file(&file);
        }
        Ok(mutation.result)
    }

    pub fn file_watch(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: FileWatchParams = parameters(params, "watch")?;
        let mut file = self.editor.stored(&request.file_id)?.clone();
        file.watching = request.watch;
        file.updated_ms = self.now_ms();
        let result = serde_json::to_value(FileWatchResult {
            file_id: file.file_id.clone(),
            watching: file.watching,
        })
        .map_err(|_| errors::internal("File watch result serialization failed"))?;
        let saved = file.clone();
        let mutation = self.mutate(connection, method::FILE_WATCH, params, |change| {
            change.save_editor_file(&saved)?;
            Ok((result, Vec::new()))
        })?;
        if !mutation.repeated {
            let file_id = file.file_id.clone();
            *self.editor.stored_mut(&file_id)? = file;
            self.editor.checked_watch(&file_id, self.now_ms());
        }
        Ok(mutation.result)
    }

    pub fn file_history(&self, params: &Value) -> Result<Value> {
        let request: FileHistoryParams = parameters(params, "history")?;
        self.editor.stored(&request.file_id)?;
        let page = self
            .store
            .file_history(
                &request.file_id,
                request.cursor.as_deref(),
                request.limit as usize,
            )
            .map_err(errors::storage)?;
        serde_json::to_value(FileHistoryResult {
            items: page.items,
            next_cursor: page.next_cursor,
        })
        .map_err(|_| errors::internal("File history result serialization failed"))
    }

    pub fn file_revert(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: FileRevertParams = parameters(params, "revert")?;
        let current = self.editor.stored(&request.file_id)?.clone();
        if current.revision != revision(&request.base_revision)? {
            return Err(errors::stale_version("File revision changed"));
        }
        let version = self
            .store
            .file_version(&request.file_id, &request.version_id)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::not_found("File version not found"))?;
        let mut file = current;
        if let Some(disk) = file.conflict_text.take() {
            file.disk_text = disk;
        }
        file.revision = next_revision(file.revision)?;
        file.draft_text = Some(version.content);
        file.draft_author = Some(self.file_author(connection)?);
        file.updated_ms = self.now_ms();
        let result = serde_json::to_value(FileRevertResult {
            file: document(file.clone()).map_err(errors::storage)?,
        })
        .map_err(|_| errors::internal("File revert result serialization failed"))?;
        let saved = file.clone();
        let mutation = self.mutate(connection, method::FILE_REVERT, params, |change| {
            change.save_editor_file(&saved)?;
            Ok((result, Vec::new()))
        })?;
        if !mutation.repeated {
            self.editor.replace(file.clone(), self.now_ms());
            self.track_language_file(&file);
        }
        Ok(mutation.result)
    }

    pub fn advance_editor(&mut self) -> Result<()> {
        for line in self.advance_editor_effects() {
            line.emit();
        }
        Ok(())
    }

    /// Runs the autosaves and disk checks that are due and returns the log
    /// records they produced. A failing effect backs off (see `schedule`), so
    /// a file that stays unreadable is retried less and less often and logged
    /// only at attempts 1, 2, 4, 8 and so on.
    pub(super) fn advance_editor_effects(&mut self) -> Vec<Line> {
        let now = self.now_ms();
        let mut records = Vec::new();
        if !self.hooks.pause_editor_autosave() {
            for file_id in self.editor.due_saves(now) {
                let outcome = self.advance_editor_save(&file_id, now);
                records.extend(self.settle_editor_effect(&file_id, Effect::Save, now, outcome));
            }
        }
        for file_id in self.editor.due_watches(now) {
            let outcome = self.advance_editor_watch(&file_id, now);
            records.extend(self.settle_editor_effect(&file_id, Effect::Watch, now, outcome));
        }
        records
    }

    fn settle_editor_effect(
        &mut self,
        file_id: &str,
        effect: Effect,
        now: i64,
        outcome: Result<()>,
    ) -> Option<Line> {
        match outcome {
            Ok(()) => self
                .editor
                .effect_succeeded(file_id, effect)
                .map(|attempts| {
                    Line::new("engine.editor_effect_recovered")
                        .field("action", effect.name())
                        .field("fileId", file_id)
                        .field("attempts", attempts)
                }),
            Err(error) => {
                // Only the class of the failure: an error's text can carry paths.
                let kind = soyeht_platform::log::Failure::failure_kind(&error);
                let failure = self.editor.effect_failed(file_id, effect, &kind, now)?;
                failure
                    .report
                    .then(|| failure_line(effect, file_id, kind, failure))
            }
        }
    }

    /// Runs a pending autosave now instead of waiting for its debounce; used
    /// before a tab closes or its file moves. A conflict is left untouched.
    pub(super) fn save_editor_now(&mut self, file_id: &str) -> Result<()> {
        let pending = self
            .editor
            .stored(file_id)
            .is_ok_and(|file| file.draft_text.is_some() && file.conflict_text.is_none());
        if !pending || self.hooks.pause_editor_autosave() {
            return Ok(());
        }
        self.advance_editor_save(file_id, self.now_ms())
    }

    fn advance_editor_save(&mut self, file_id: &str, now: i64) -> Result<()> {
        let file = self.editor.stored(file_id)?.clone();
        let Some(draft) = file.draft_text.clone() else {
            return Ok(());
        };
        if file.conflict_text.is_some() {
            return Ok(());
        }
        let workspace = model::workspace(&self.store, &file.workspace_id)?;
        let disk = self.editor.current_disk(workspace.path.as_ref(), &file)?;
        if disk != file.disk_text {
            return self.apply_disk_change(file, disk, now);
        }
        if !self
            .editor
            .save_disk(workspace.path.as_ref(), &file, &file.disk_text, &draft)?
        {
            let disk = self.editor.current_disk(workspace.path.as_ref(), &file)?;
            return self.apply_disk_change(file, disk, now);
        }
        self.finish_editor_save(file, now)
    }

    fn advance_editor_watch(&mut self, file_id: &str, now: i64) -> Result<()> {
        let file = self.editor.stored(file_id)?.clone();
        self.editor.checked_watch(file_id, now);
        let workspace = model::workspace(&self.store, &file.workspace_id)?;
        let disk = self.editor.current_disk(workspace.path.as_ref(), &file)?;
        let known = file.conflict_text.as_ref().unwrap_or(&file.disk_text);
        if disk == *known {
            return Ok(());
        }
        self.apply_disk_change(file, disk, now)
    }

    fn apply_disk_change(&mut self, file: EditorFile, disk: String, now: i64) -> Result<()> {
        if file.draft_text.as_deref() == Some(&disk) {
            return self.finish_editor_save(file, now);
        }
        if file.draft_text.is_some() {
            self.record_editor_conflict(file, disk, now)
        } else {
            self.reload_editor_file(file, disk, now)
        }
    }

    fn finish_editor_save(&mut self, mut file: EditorFile, now: i64) -> Result<()> {
        let content = file
            .draft_text
            .take()
            .ok_or_else(|| errors::internal("Due editor save has no draft"))?;
        let author = file
            .draft_author
            .take()
            .ok_or_else(|| errors::internal("Due editor save has no author"))?;
        file.disk_revision = next_revision(file.disk_revision)?;
        file.disk_text = content.clone();
        file.conflict_text = None;
        file.updated_ms = now;
        let version = stored_version(&file, content, author.clone(), self.timestamp());
        let wire = document(file.clone()).map_err(errors::storage)?;
        let result = json!({"file":wire});
        let events = vec![(event::FILE_SAVED.into(), json!({"file":wire,"by":author}))];
        self.persist_editor_system("save", file, Some(version), result, events, true)
    }

    fn reload_editor_file(&mut self, mut file: EditorFile, disk: String, now: i64) -> Result<()> {
        let changed = changed_lines(&file.disk_text, &disk);
        file.revision = next_revision(file.revision)?;
        file.disk_revision = next_revision(file.disk_revision)?;
        file.disk_text = disk.clone();
        file.updated_ms = now;
        let author = filesystem_author();
        let version = stored_version(&file, disk, author, self.timestamp());
        let event = json!({
            "fileId":file.file_id,
            "diskRevision":file.disk_revision.to_string(),
            "changedLines":changed
        });
        self.persist_editor_system(
            "reload",
            file,
            Some(version),
            json!({}),
            vec![(event::FILE_CHANGED_ON_DISK.into(), event)],
            false,
        )
    }

    fn record_editor_conflict(
        &mut self,
        mut file: EditorFile,
        disk: String,
        now: i64,
    ) -> Result<()> {
        let base = file.disk_revision;
        file.disk_revision = next_revision(file.disk_revision)?;
        file.conflict_text = Some(disk);
        file.updated_ms = now;
        let event = json!({
            "fileId":file.file_id,
            "baseRevision":base.to_string(),
            "draftRevision":file.revision.to_string(),
            "diskRevision":file.disk_revision.to_string()
        });
        self.persist_editor_system(
            "conflict",
            file,
            None,
            json!({}),
            vec![(event::FILE_CONFLICT.into(), event)],
            true,
        )
    }

    fn persist_editor_system(
        &mut self,
        action: &str,
        file: EditorFile,
        version: Option<StoredFileVersion>,
        result: Value,
        events: Vec<(String, Value)>,
        preserve_syntax: bool,
    ) -> Result<()> {
        let key = format!("{action}:{}:{}", file.file_id, file.disk_revision);
        let saved = file.clone();
        let mutation = StorePort::mutate(&mut self.store, "engine-editor", &key, &key, |change| {
            change.save_editor_file(&saved)?;
            if let Some(version) = &version {
                change.record_file_version(version)?;
            }
            Ok((result, events))
        })
        .map_err(errors::storage)?;
        if preserve_syntax {
            self.editor
                .replace_preserving_syntax(file.clone(), self.now_ms());
        } else {
            self.editor.replace(file.clone(), self.now_ms());
        }
        self.publish(&mutation.events)?;
        self.track_language_file(&file);
        Ok(())
    }

    pub(crate) fn file_author(&self, connection: &str) -> Result<FileAuthor> {
        let actor = self
            .clients
            .get(connection)
            .and_then(|client| client.actor.clone())
            .ok_or_else(|| errors::unauthorized("File connection is not authenticated"))?;
        let name = match actor.role {
            Role::HumanLocal | Role::HumanDevice => "you".into(),
            Role::Agent | Role::Chief => self
                .panes
                .get(actor.pane_id.as_deref().unwrap_or_default())
                .map(|pane| pane.stored.pane.name.clone())
                .ok_or_else(|| errors::unauthorized("Bound agent pane is unavailable"))?,
        };
        Ok(FileAuthor { actor, name })
    }
}

pub(super) fn parameters<T: serde::de::DeserializeOwned>(params: &Value, name: &str) -> Result<T> {
    serde_json::from_value(params.clone())
        .map_err(|_| errors::invalid(format!("Invalid file {name} parameters")))
}

pub(super) fn revision(value: &str) -> Result<u64> {
    value
        .parse()
        .map_err(|_| errors::invalid("Invalid file revision"))
}

pub(super) fn next_revision(current: u64) -> Result<u64> {
    current
        .checked_add(1)
        .ok_or_else(|| errors::internal("File revision exhausted"))
}

pub(crate) fn stored_version(
    file: &EditorFile,
    content: String,
    author: FileAuthor,
    created_at: String,
) -> StoredFileVersion {
    StoredFileVersion {
        version: soyeht_protocol::FileVersion {
            version_id: Uuid::new_v4().to_string(),
            file_id: file.file_id.clone(),
            revision: file.revision.to_string(),
            created_at,
            author,
            byte_count: content.len().to_string(),
        },
        content,
    }
}

pub(super) fn filesystem_author() -> FileAuthor {
    FileAuthor {
        actor: Actor {
            role: Role::HumanLocal,
            client: "filesystem".into(),
            device_id: None,
            pane_id: None,
        },
        name: "external change".into(),
    }
}

fn changed_lines(before: &str, after: &str) -> Vec<u32> {
    let before: Vec<_> = before.split('\n').collect();
    let after: Vec<_> = after.split('\n').collect();
    let count = before.len().max(after.len());
    (0..count)
        .filter(|index| before.get(*index) != after.get(*index))
        .take(soyeht_protocol::limits::EDITOR_VIEWPORT_LINES)
        .filter_map(|index| u32::try_from(index).ok())
        .collect()
}

fn failure_line(effect: Effect, file_id: &str, kind: String, failure: Failure) -> Line {
    Line::new("engine.editor_effect_failed")
        .field("action", effect.name())
        .field("fileId", file_id)
        .field("kind", kind)
        .field("attempts", failure.attempts)
        .field("retryInMs", failure.retry_in_ms)
}

use super::persistence::{filesystem_author, next_revision, stored_version};
use crate::{
    core::Core,
    errors::{self, Result},
    model::{RunningPane, StoredPane},
    panes::persist_new_pane,
    screen::Screen,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    EditorContentSource, FileOpenParams, FileOpenResult, FileReadParams, FileReadResult, Pane,
    PaneMode, PaneState, StateSource,
};
use soyeht_store::editor::{EditorFile, StoredFileVersion, document};
use std::path::Path;
use uuid::Uuid;

const INITIAL_VIEWPORT_LINES: usize = 200;

struct ReattachRequest<'a> {
    connection: &'a str,
    method: &'a str,
    params: &'a Value,
    line: Option<u32>,
    pane_only: bool,
    workspace: &'a soyeht_protocol::Workspace,
    canonical: &'a Path,
}

impl Core {
    pub fn file_open(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request: FileOpenParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid file open parameters"))?;
        self.open_editor(connection, "file.open", params, request, false)
    }

    pub fn editor_pane_create(&mut self, connection: &str, params: &Value) -> Result<Value> {
        let request = FileOpenParams {
            workspace_id: errors::param_str(params, "workspaceId")?.into(),
            path: errors::param_str(params, "path")?.into(),
            line: None,
            column: None,
            idempotency_key: errors::param_str(params, "idempotencyKey")?.into(),
        };
        self.open_editor(connection, "pane.create", params, request, true)
    }

    fn open_editor(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
        request: FileOpenParams,
        pane_only: bool,
    ) -> Result<Value> {
        let workspace = crate::model::workspace(&self.store, &request.workspace_id)?;
        let resolved = self
            .editor
            .resolve(Path::new(&workspace.path), &request.path)?;
        if let Some(file) = self
            .store
            .editor_file_for_path(&request.workspace_id, &resolved.relative)
            .map_err(errors::storage)?
        {
            if file.pane_id.as_deref().is_some_and(|pane_id| {
                self.panes
                    .get(pane_id)
                    .is_some_and(|pane| !pane.stored.closed)
            }) {
                let result = self.open_result(&file, request.line)?;
                let reply = open_reply(result, pane_only)?;
                return Ok(self
                    .mutate(connection, method, params, |_| Ok((reply, vec![])))?
                    .result);
            }
            return self.reattach_editor(
                ReattachRequest {
                    connection,
                    method,
                    params,
                    line: request.line,
                    pane_only,
                    workspace: &workspace,
                    canonical: &resolved.canonical,
                },
                file,
            );
        }
        let text = self.editor.read_disk(&resolved.canonical)?;
        let language = super::service::language(&resolved.canonical);
        let pane = editor_pane(&workspace, &resolved.relative);
        let saved = StoredPane {
            pane: pane.clone(),
            closed: false,
            instance_key: Uuid::new_v4().to_string(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        };
        let file = EditorFile {
            file_id: Uuid::new_v4().to_string(),
            pane_id: Some(pane.pane_id.clone()),
            workspace_id: workspace.workspace_id,
            relative_path: resolved.relative,
            language,
            revision: 1,
            disk_revision: 1,
            disk_text: text,
            draft_text: None,
            draft_author: None,
            conflict_text: None,
            watching: true,
            updated_ms: self.now_ms(),
        };
        let (opened, syntax) = initial_open_result(&file, &pane, request.line)?;
        let reply = open_reply(opened, pane_only)?;
        let screen = Screen::open(
            &self.directory.join("sessions").join(&saved.pane.pane_id),
            false,
        )?;
        let mutation = self.mutate(connection, method, params, |change| {
            persist_new_pane(change, &saved, None)?;
            change.save_editor_file(&file)?;
            Ok((
                reply,
                vec![("pane.created".into(), json!({"pane":saved.pane}))],
            ))
        })?;
        if !mutation.repeated {
            self.panes.insert(
                pane.pane_id.clone(),
                RunningPane {
                    stored: saved,
                    screen,
                },
            );
            self.editor.insert_parsed(file, syntax);
        }
        Ok(mutation.result)
    }

    fn reattach_editor(&mut self, request: ReattachRequest<'_>, file: EditorFile) -> Result<Value> {
        let disk = self.editor.read_disk(request.canonical)?;
        let (mut file, version) =
            reconcile_reopened_file(file, disk, self.now_ms(), self.timestamp())?;
        let pane = editor_pane(request.workspace, &file.relative_path);
        file.pane_id = Some(pane.pane_id.clone());
        file.watching = true;
        let saved = StoredPane {
            pane: pane.clone(),
            closed: false,
            instance_key: Uuid::new_v4().to_string(),
            instance_id: None,
            supervisor_id: None,
            sequence_base: 0,
        };
        let (opened, syntax) = initial_open_result(&file, &pane, request.line)?;
        let reply = open_reply(opened, request.pane_only)?;
        let screen = Screen::open(
            &self.directory.join("sessions").join(&saved.pane.pane_id),
            false,
        )?;
        let mutation = self.mutate(
            request.connection,
            request.method,
            request.params,
            |change| {
                persist_new_pane(change, &saved, None)?;
                change.save_editor_file(&file)?;
                if let Some(version) = &version {
                    change.record_file_version(version)?;
                }
                Ok((
                    reply,
                    vec![("pane.created".into(), json!({"pane":saved.pane}))],
                ))
            },
        )?;
        if !mutation.repeated {
            self.panes.insert(
                pane.pane_id.clone(),
                RunningPane {
                    stored: saved,
                    screen,
                },
            );
            self.editor.insert_parsed(file, syntax);
        }
        Ok(mutation.result)
    }

    pub fn file_read(&mut self, params: &Value) -> Result<Value> {
        let request: FileReadParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid file read parameters"))?;
        let file = self.editor.stored(&request.file_id)?.clone();
        if request
            .revision
            .as_deref()
            .is_some_and(|revision| revision.parse::<u64>().ok() != Some(file.revision))
        {
            return Err(errors::stale_version("File revision changed"));
        }
        let (source, text) = self.read_source(&file, &request)?;
        let lines = self.editor.viewport(
            &file.file_id,
            &text,
            file.language.clone(),
            request.from_line as usize,
            request.line_count as usize,
        )?;
        let to_line = request.from_line.saturating_add(lines.len() as u32);
        serde_json::to_value(FileReadResult {
            file: document(file).map_err(errors::storage)?,
            source,
            from_line: request.from_line,
            to_line,
            lines,
        })
        .map_err(|_| errors::internal("File read result serialization failed"))
    }

    fn read_source(
        &self,
        file: &EditorFile,
        request: &FileReadParams,
    ) -> Result<(EditorContentSource, String)> {
        let source = request
            .source
            .clone()
            .unwrap_or(EditorContentSource::Buffer);
        if source != EditorContentSource::History && request.version_id.is_some() {
            return Err(errors::invalid("versionId is only valid for history reads"));
        }
        let text = match source {
            EditorContentSource::Buffer => file
                .draft_text
                .clone()
                .unwrap_or_else(|| file.disk_text.clone()),
            EditorContentSource::Disk => file
                .conflict_text
                .clone()
                .unwrap_or_else(|| file.disk_text.clone()),
            EditorContentSource::Draft => file
                .draft_text
                .clone()
                .ok_or_else(|| errors::not_found("File has no draft"))?,
            EditorContentSource::History => {
                let version = request
                    .version_id
                    .as_deref()
                    .ok_or_else(|| errors::invalid("History reads require versionId"))?;
                self.store
                    .file_version(&file.file_id, version)
                    .map_err(errors::storage)?
                    .ok_or_else(|| errors::not_found("File version not found"))?
                    .content
            }
        };
        Ok((source, text))
    }

    fn open_result(&self, file: &EditorFile, line: Option<u32>) -> Result<FileOpenResult> {
        let pane = self
            .panes
            .get(
                file.pane_id
                    .as_deref()
                    .ok_or_else(|| errors::internal("Open file has no pane"))?,
            )
            .ok_or_else(|| errors::internal("Open file has no pane"))?
            .stored
            .pane
            .clone();
        initial_open_result(file, &pane, line).map(|(result, _)| result)
    }
}

fn reconcile_reopened_file(
    mut file: EditorFile,
    disk: String,
    now_ms: i64,
    created_at: String,
) -> Result<(EditorFile, Option<StoredFileVersion>)> {
    let known = file.conflict_text.as_ref().unwrap_or(&file.disk_text);
    if file.draft_text.as_deref() == Some(&disk) {
        let author = file
            .draft_author
            .take()
            .ok_or_else(|| errors::internal("Reopened draft has no author"))?;
        file.draft_text = None;
        file.conflict_text = None;
        file.disk_text = disk.clone();
        file.disk_revision = next_revision(file.disk_revision)?;
        file.updated_ms = now_ms;
        let version = stored_version(&file, disk, author, created_at);
        return Ok((file, Some(version)));
    }
    if disk == *known {
        return Ok((file, None));
    }
    file.disk_revision = next_revision(file.disk_revision)?;
    file.updated_ms = now_ms;
    if file.draft_text.is_some() {
        file.conflict_text = Some(disk);
        return Ok((file, None));
    }
    file.revision = next_revision(file.revision)?;
    file.disk_text = disk.clone();
    let version = stored_version(&file, disk, filesystem_author(), created_at);
    Ok((file, Some(version)))
}

fn open_reply(result: FileOpenResult, pane_only: bool) -> Result<Value> {
    if pane_only {
        Ok(json!({"paneId":result.pane.pane_id}))
    } else {
        serde_json::to_value(result)
            .map_err(|_| errors::internal("File open result serialization failed"))
    }
}

fn initial_open_result(
    file: &EditorFile,
    pane: &Pane,
    line: Option<u32>,
) -> Result<(FileOpenResult, soyeht_editor::SyntaxDocument)> {
    let text = file.draft_text.as_deref().unwrap_or(&file.disk_text);
    let syntax = soyeht_editor::SyntaxDocument::new(text, file.language.clone())?;
    let last = syntax.buffer().line_count().saturating_sub(1);
    let from = (line.unwrap_or(0) as usize).min(last);
    let lines = soyeht_editor::project(&syntax, from, INITIAL_VIEWPORT_LINES)?;
    let viewport = FileReadResult {
        file: document(file.clone()).map_err(errors::storage)?,
        source: if file.draft_text.is_some() {
            EditorContentSource::Draft
        } else {
            EditorContentSource::Disk
        },
        from_line: from as u32,
        to_line: from.saturating_add(lines.len()) as u32,
        lines,
    };
    Ok((
        FileOpenResult {
            file: viewport.file.clone(),
            pane: pane.clone(),
            viewport,
        },
        syntax,
    ))
}

fn editor_pane(workspace: &soyeht_protocol::Workspace, relative_path: &str) -> Pane {
    let name = Path::new(relative_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(relative_path);
    Pane {
        pane_id: Uuid::new_v4().to_string(),
        workspace_id: workspace.workspace_id.clone(),
        name: name.into(),
        cwd: workspace.path.clone(),
        mode: PaneMode::Editor,
        state: PaneState::Idle,
        source: StateSource::Protocol,
        confidence: 1.0,
        phrase: format!("Editing {name}"),
        generation: 1,
        cols: 0,
        rows: 0,
        record: false,
        pid: None,
        stream_id: None,
        agent: None,
    }
}

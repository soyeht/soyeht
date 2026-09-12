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
use soyeht_store::editor::{EditorFile, document};
use std::path::Path;
use uuid::Uuid;

const INITIAL_VIEWPORT_LINES: usize = 200;

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
            .editor_files()
            .map_err(errors::storage)?
            .into_iter()
            .find(|file| {
                file.workspace_id == request.workspace_id && file.relative_path == resolved.relative
            })
        {
            let result = self.open_result(&file, request.line)?;
            let reply = open_reply(result, pane_only)?;
            return Ok(self
                .mutate(connection, method, params, |_| Ok((reply, vec![])))?
                .result);
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
            pane_id: pane.pane_id.clone(),
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
        let reply = open_reply(initial_open_result(&file, &pane, request.line)?, pane_only)?;
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
            self.editor.insert(file);
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
            file: document(file),
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
            EditorContentSource::Disk => file.disk_text.clone(),
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
            .get(&file.pane_id)
            .ok_or_else(|| errors::internal("Open file has no pane"))?
            .stored
            .pane
            .clone();
        initial_open_result(file, &pane, line)
    }
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
) -> Result<FileOpenResult> {
    let text = file.draft_text.as_deref().unwrap_or(&file.disk_text);
    let syntax = soyeht_editor::SyntaxDocument::new(text, file.language.clone())?;
    let last = syntax.buffer().line_count().saturating_sub(1);
    let from = (line.unwrap_or(0) as usize).min(last);
    let lines = soyeht_editor::project(&syntax, from, INITIAL_VIEWPORT_LINES)?;
    let viewport = FileReadResult {
        file: document(file.clone()),
        source: if file.draft_text.is_some() {
            EditorContentSource::Draft
        } else {
            EditorContentSource::Disk
        },
        from_line: from as u32,
        to_line: from.saturating_add(lines.len()) as u32,
        lines,
    };
    Ok(FileOpenResult {
        file: viewport.file.clone(),
        pane: pane.clone(),
        viewport,
    })
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

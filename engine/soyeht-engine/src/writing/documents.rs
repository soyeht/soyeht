use crate::{
    core::Core,
    errors::{self, Result},
    model::{RunningPane, StoredPane},
    panes::persist_new_pane,
    screen::Screen,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Pane, PaneMode, PaneState, StateSource, WritingDocument, WritingDocumentOpenParams,
    WritingDocumentOpenResult, WritingFont, WritingKind, WritingPreferencesParams,
    WritingPreferencesResult, WritingSection,
};
use soyeht_store::{
    editor::{EditorFile, document},
    writing::{WritingDocumentRecord, WritingSidecar},
};
use std::path::Path;
use uuid::Uuid;

struct OpenContext<'a> {
    connection: &'a str,
    method: &'a str,
    params: &'a Value,
    workspace: &'a soyeht_protocol::Workspace,
    canonical: &'a Path,
    kind: WritingKind,
    section: WritingSection,
    pane_only: bool,
}

impl Core {
    pub(super) fn writing_document_open(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Value> {
        let request: WritingDocumentOpenParams = parameters(params, "document open")?;
        self.open_writing(connection, "writing.document.open", params, request, false)
    }

    pub(crate) fn writing_pane_create(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Value> {
        let request: soyeht_protocol::PaneCreateParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid writing pane parameters"))?;
        let kind = request
            .writing_kind
            .ok_or_else(|| errors::invalid("Writing pane requires writingKind"))?;
        let section = section_for(&kind);
        if request.writing_section.as_ref() != Some(&section) {
            return Err(errors::writing_kind_refused(
                "Writing pane kind and section do not match",
            ));
        }
        let request = WritingDocumentOpenParams {
            workspace_id: request.workspace_id,
            path: request
                .path
                .ok_or_else(|| errors::invalid("Writing pane requires path"))?,
            kind,
            idempotency_key: request.idempotency_key,
        };
        self.open_writing(connection, "pane.create", params, request, true)
    }

    fn open_writing(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
        request: WritingDocumentOpenParams,
        pane_only: bool,
    ) -> Result<Value> {
        let section = section_for(&request.kind);
        validate_writing_path(&request.path, &request.kind, &section)?;
        let workspace = crate::model::workspace(&self.store, &request.workspace_id)?;
        let resolved = self
            .editor
            .resolve(Path::new(&workspace.path), &request.path)?;
        let existing = self
            .store
            .editor_file_for_path(&request.workspace_id, &resolved.relative)
            .map_err(errors::storage)?;
        let context = OpenContext {
            connection,
            method,
            params,
            workspace: &workspace,
            canonical: &resolved.canonical,
            kind: request.kind,
            section,
            pane_only,
        };
        if let Some(file) = existing {
            return self.open_existing_writing(context, file);
        }
        self.open_new_writing(context, resolved.relative)
    }

    pub(super) fn writing_preferences(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Value> {
        let request: WritingPreferencesParams = parameters(params, "preferences")?;
        let file = self.editor.stored(&request.file_id)?.clone();
        require_revision(&request.base_revision, file.revision)?;
        let mut record = self.writing_record(&file.file_id)?;
        if let Some(font) = request.font {
            record.font = font;
        }
        if let Some(focus_mode) = request.focus_mode {
            record.focus_mode = focus_mode;
        }
        record.updated_ms = self.now_ms();
        let result = serde_json::to_value(WritingPreferencesResult {
            document: public_document(&file, &record)?,
        })
        .map_err(|_| errors::internal("Writing preferences result cannot be encoded"))?;
        let saved = record.clone();
        let event = changed_event(&file, &record)?;
        self.mutate(connection, "writing.preferences.set", params, |change| {
            change.save_writing_document(&saved)?;
            Ok((result, vec![("writing.changed".into(), event)]))
        })
        .map(|mutation| mutation.result)
    }

    pub(super) fn writing_record(&self, file_id: &str) -> Result<WritingDocumentRecord> {
        self.store
            .writing_document(file_id)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::not_found("Writing document not found"))
    }

    fn open_existing_writing(
        &mut self,
        context: OpenContext<'_>,
        file: EditorFile,
    ) -> Result<Value> {
        if let Some(record) = self
            .store
            .writing_document(&file.file_id)
            .map_err(errors::storage)?
        {
            require_kind(&record, &context.kind, &context.section)?;
        }
        let active = file
            .pane_id
            .as_deref()
            .and_then(|pane_id| self.panes.get(pane_id).filter(|pane| !pane.stored.closed));
        if let Some(active) = active {
            require_writing_pane(&active.stored.pane, &context.kind, &context.section)?;
            return self.reply_for_active(context, file);
        }
        self.reattach_writing(context, file)
    }

    fn reply_for_active(&mut self, context: OpenContext<'_>, file: EditorFile) -> Result<Value> {
        let pane = active_pane(self, &file)?.clone();
        let record = self.record_or_default(file.file_id.clone(), context.kind, context.section)?;
        let (opened, _) = crate::editor::api::initial_open_result(&file, &pane, None, None)?;
        let result = open_result(&file, &record, opened, context.pane_only)?;
        let saved = record.clone();
        self.mutate(
            context.connection,
            context.method,
            context.params,
            |change| {
                change.save_writing_document(&saved)?;
                Ok((result, Vec::new()))
            },
        )
        .map(|mutation| mutation.result)
    }

    fn reattach_writing(&mut self, context: OpenContext<'_>, file: EditorFile) -> Result<Value> {
        let disk = self.editor.read_disk(context.canonical)?;
        let (mut file, version) = crate::editor::api::reconcile_reopened_file(
            file,
            disk,
            self.now_ms(),
            self.timestamp(),
        )?;
        let pane = writing_pane(
            context.workspace,
            &file.relative_path,
            context.kind.clone(),
            context.section.clone(),
        );
        file.pane_id = Some(pane.pane_id.clone());
        file.watching = true;
        let saved_pane = stored_pane(pane.clone());
        let record = self.record_or_default(file.file_id.clone(), context.kind, context.section)?;
        let (opened, syntax) = crate::editor::api::initial_open_result(&file, &pane, None, None)?;
        let result = open_result(&file, &record, opened, context.pane_only)?;
        let screen = self.open_pane_screen(&pane.pane_id)?;
        let saved_file = file.clone();
        let saved_record = record.clone();
        let mutation = self.mutate(
            context.connection,
            context.method,
            context.params,
            |change| {
                persist_new_pane(change, &saved_pane, None)?;
                change.save_editor_file(&saved_file)?;
                change.save_writing_document(&saved_record)?;
                if let Some(version) = &version {
                    change.record_file_version(version)?;
                }
                Ok((
                    result,
                    vec![("pane.created".into(), json!({"pane":saved_pane.pane}))],
                ))
            },
        )?;
        if !mutation.repeated {
            self.install_open_writing(saved_pane, screen, file, syntax);
        }
        Ok(mutation.result)
    }

    fn open_new_writing(&mut self, context: OpenContext<'_>, relative: String) -> Result<Value> {
        let text = self.editor.read_disk(context.canonical)?;
        let pane = writing_pane(
            context.workspace,
            &relative,
            context.kind.clone(),
            context.section.clone(),
        );
        let saved_pane = stored_pane(pane.clone());
        let file = new_file(
            context.workspace,
            &pane,
            relative,
            text,
            context.canonical,
            self.now_ms(),
        );
        let record = self.record_or_default(file.file_id.clone(), context.kind, context.section)?;
        let (opened, syntax) = crate::editor::api::initial_open_result(&file, &pane, None, None)?;
        let result = open_result(&file, &record, opened, context.pane_only)?;
        let screen = self.open_pane_screen(&pane.pane_id)?;
        let saved_file = file.clone();
        let saved_record = record.clone();
        let mutation = self.mutate(
            context.connection,
            context.method,
            context.params,
            |change| {
                persist_new_pane(change, &saved_pane, None)?;
                change.save_editor_file(&saved_file)?;
                change.save_writing_document(&saved_record)?;
                Ok((
                    result,
                    vec![("pane.created".into(), json!({"pane":saved_pane.pane}))],
                ))
            },
        )?;
        if !mutation.repeated {
            self.install_open_writing(saved_pane, screen, file, syntax);
        }
        Ok(mutation.result)
    }

    fn record_or_default(
        &self,
        file_id: String,
        kind: WritingKind,
        section: WritingSection,
    ) -> Result<WritingDocumentRecord> {
        if let Some(record) = self
            .store
            .writing_document(&file_id)
            .map_err(errors::storage)?
        {
            require_kind(&record, &kind, &section)?;
            return Ok(record);
        }
        Ok(WritingDocumentRecord {
            file_id,
            kind,
            section,
            sidecar_revision: 0,
            font: WritingFont::ReadingSerif,
            focus_mode: false,
            sidecar: WritingSidecar::default(),
            updated_ms: self.now_ms(),
        })
    }

    fn open_pane_screen(&self, pane_id: &str) -> Result<Screen> {
        Screen::open(&self.directory.join("sessions").join(pane_id), false)
    }

    fn install_open_writing(
        &mut self,
        pane: StoredPane,
        screen: Screen,
        file: EditorFile,
        syntax: soyeht_editor::SyntaxDocument,
    ) {
        self.panes.insert(
            pane.pane.pane_id.clone(),
            RunningPane {
                stored: pane,
                screen,
            },
        );
        self.editor.insert_parsed(file, syntax);
    }
}

pub(super) fn public_document(
    file: &EditorFile,
    record: &WritingDocumentRecord,
) -> Result<WritingDocument> {
    Ok(WritingDocument {
        file: document(file.clone()).map_err(errors::storage)?,
        kind: record.kind.clone(),
        section: record.section.clone(),
        sidecar_revision: record.sidecar_revision.to_string(),
        font: record.font.clone(),
        focus_mode: record.focus_mode,
    })
}

pub(super) fn require_revision(value: &str, current: u64) -> Result<()> {
    if value.parse::<u64>().ok() != Some(current) {
        return Err(errors::stale_version("Writing document revision changed"));
    }
    Ok(())
}

fn validate_writing_path(path: &str, kind: &WritingKind, section: &WritingSection) -> Result<()> {
    let title = Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    soyeht_writing::section_entries(
        section.clone(),
        vec![soyeht_writing::SectionEntryInput {
            path: path.to_owned(),
            title,
            file_id: None,
            modified_at: "1970-01-01T00:00:00.000Z".into(),
            byte_count: 0,
        }],
    )?;
    if soyeht_writing::kind_for(section) != *kind {
        return Err(errors::Error::Writing(
            soyeht_writing::Error::InvalidSectionPath,
        ));
    }
    Ok(())
}

fn section_for(kind: &WritingKind) -> WritingSection {
    match kind {
        WritingKind::Text => WritingSection::Texts,
        WritingKind::Presentation => WritingSection::Presentations,
        WritingKind::Screenplay => WritingSection::Screenplays,
    }
}

fn require_kind(
    record: &WritingDocumentRecord,
    kind: &WritingKind,
    section: &WritingSection,
) -> Result<()> {
    if &record.kind != kind || &record.section != section {
        return Err(errors::writing_kind_refused(
            "Writing document kind cannot change",
        ));
    }
    Ok(())
}

fn require_writing_pane(pane: &Pane, kind: &WritingKind, section: &WritingSection) -> Result<()> {
    if pane.mode != PaneMode::Writing
        || pane.writing_kind.as_ref() != Some(kind)
        || pane.writing_section.as_ref() != Some(section)
    {
        return Err(errors::writing_kind_refused(
            "File is already owned by another editor surface",
        ));
    }
    Ok(())
}

fn active_pane<'a>(core: &'a Core, file: &EditorFile) -> Result<&'a Pane> {
    file.pane_id
        .as_deref()
        .and_then(|pane_id| core.panes.get(pane_id))
        .map(|pane| &pane.stored.pane)
        .ok_or_else(|| errors::internal("Open writing document has no pane"))
}

fn new_file(
    workspace: &soyeht_protocol::Workspace,
    pane: &Pane,
    relative: String,
    text: String,
    canonical: &Path,
    now_ms: i64,
) -> EditorFile {
    EditorFile {
        file_id: Uuid::new_v4().to_string(),
        pane_id: Some(pane.pane_id.clone()),
        workspace_id: workspace.workspace_id.clone(),
        relative_path: relative,
        language: crate::editor::language(canonical),
        revision: 1,
        disk_revision: 1,
        disk_text: text,
        draft_text: None,
        draft_author: None,
        conflict_text: None,
        watching: true,
        updated_ms: now_ms,
    }
}

fn writing_pane(
    workspace: &soyeht_protocol::Workspace,
    relative: &str,
    kind: WritingKind,
    section: WritingSection,
) -> Pane {
    let name = Path::new(relative)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(relative);
    Pane {
        pane_id: Uuid::new_v4().to_string(),
        workspace_id: workspace.workspace_id.clone(),
        name: name.into(),
        cwd: workspace.path.clone(),
        mode: PaneMode::Writing,
        state: PaneState::Idle,
        source: StateSource::Protocol,
        confidence: 1.0,
        phrase: format!("Writing {name}"),
        generation: 1,
        cols: 0,
        rows: 0,
        record: false,
        pid: None,
        stream_id: None,
        agent: None,
        url: None,
        title: None,
        viewport: None,
        incognito: None,
        writing_kind: Some(kind),
        writing_section: Some(section),
        web_session: None,
    }
}

fn stored_pane(pane: Pane) -> StoredPane {
    StoredPane {
        pane,
        closed: false,
        instance_key: Uuid::new_v4().to_string(),
        instance_id: None,
        supervisor_id: None,
        sequence_base: 0,
    }
}

fn open_result(
    file: &EditorFile,
    record: &WritingDocumentRecord,
    opened: soyeht_protocol::FileOpenResult,
    pane_only: bool,
) -> Result<Value> {
    if pane_only {
        return Ok(json!({"paneId":opened.pane.pane_id}));
    }
    serde_json::to_value(WritingDocumentOpenResult {
        document: public_document(file, record)?,
        pane: opened.pane,
        viewport: opened.viewport,
    })
    .map_err(|_| errors::internal("Writing open result cannot be encoded"))
}

fn changed_event(file: &EditorFile, record: &WritingDocumentRecord) -> Result<Value> {
    let pane_id = file
        .pane_id
        .clone()
        .ok_or_else(|| errors::internal("Writing document has no pane"))?;
    Ok(json!({
        "fileId":file.file_id,
        "paneId":pane_id,
        "revision":file.revision.to_string(),
        "sidecarRevision":record.sidecar_revision.to_string()
    }))
}

fn parameters<T: serde::de::DeserializeOwned>(params: &Value, name: &str) -> Result<T> {
    serde_json::from_value(params.clone())
        .map_err(|_| errors::invalid(format!("Invalid writing {name} parameters")))
}

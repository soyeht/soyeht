use super::documents::{changed_event, public_document, require_revision};
use crate::{core::Core, errors, errors::Result};
use serde_json::Value;
use soyeht_protocol::{
    Actor, FileAuthor, PaneState, Role, TextEdit, TextPosition, TextRange, WritingLineVariants,
    WritingVariant, WritingVariantAddParams, WritingVariantRemoveParams,
    WritingVariantRequestParams, WritingVariantRequestResult, WritingVariantResult,
    WritingVariantSelectParams, event, method,
};
use soyeht_store::{editor::EditorFile, writing::WritingDocumentRecord};
use soyeht_writing::VariantSet;
use uuid::Uuid;

impl Core {
    pub(super) fn writing_variant_request(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Value> {
        if let Some(result) =
            self.repeated_mutation(connection, method::WRITING_VARIANT_REQUEST, params)?
        {
            return Ok(result);
        }
        let request: WritingVariantRequestParams = parameters(params, "variant request")?;
        let author = self.file_author(connection)?;
        require_person(&author)?;
        let (file, _, line) =
            self.variant_context(&request.file_id, &request.base_revision, &request.line_id)?;
        let request_id = Uuid::new_v4().to_string();
        if !self.variant_agent_is_live(&request.agent_pane_id, &file.workspace_id)? {
            return self.finish_variant_request(connection, params, request_id, false);
        }
        validate_canonical(&request.line_id, line.number, &line.text, author)?;
        let prompt = variant_prompt(&request, &line.text)?;
        let mut state = self.agent_state(&request.agent_pane_id)?;
        let queued = state.enqueue_feedback(
            format!("task-feedback:writing-variant:{request_id}"),
            prompt,
            self.timestamp(),
        )?;
        let result = variant_request_result(request_id, true)?;
        let pane = self
            .panes
            .get(&request.agent_pane_id)
            .ok_or_else(|| errors::not_found("Variant agent pane is unavailable"))?
            .stored
            .clone();
        let events = vec![crate::model::pane_queued(
            &request.agent_pane_id,
            state.queue.len(),
            vec![queued.prompt_id.clone()],
            false,
        )?];
        Ok(self
            .commit_agent(
                pane,
                state,
                crate::agents::Changes {
                    events,
                    ..crate::agents::Changes::default()
                },
                Some((connection, method::WRITING_VARIANT_REQUEST, params)),
                result,
            )?
            .result)
    }

    pub(super) fn writing_variant_add(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Value> {
        if let Some(result) =
            self.repeated_mutation(connection, method::WRITING_VARIANT_ADD, params)?
        {
            return Ok(result);
        }
        let request: WritingVariantAddParams = parameters(params, "variant add")?;
        let author = self.file_author(connection)?;
        require_agent(&author)?;
        let (file, mut record, line) =
            self.variant_context(&request.file_id, &request.base_revision, &request.line_id)?;
        authorize_agent_workspace(self, &author, &file.workspace_id)?;
        let mut variants = take_or_create_variants(&mut record, &line, writer_author())?;
        variants.add(WritingVariant {
            variant_id: Uuid::new_v4().to_string(),
            text: request.text,
            author: author.clone(),
        })?;
        save_variant_sidecar(
            self,
            MutationContext {
                connection,
                method: method::WRITING_VARIANT_ADD,
                params,
            },
            VariantPersistence {
                file,
                record,
                variants,
                author,
            },
        )
    }

    pub(super) fn writing_variant_select(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Value> {
        let request: WritingVariantSelectParams = parameters(params, "variant select")?;
        self.change_variant_choice(
            MutationContext {
                connection,
                method: method::WRITING_VARIANT_SELECT,
                params,
            },
            VariantChoice {
                file_id: &request.file_id,
                line_id: &request.line_id,
                variant_id: &request.variant_id,
                base_revision: &request.base_revision,
                remove: false,
            },
        )
    }

    pub(super) fn writing_variant_remove(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Value> {
        let request: WritingVariantRemoveParams = parameters(params, "variant remove")?;
        self.change_variant_choice(
            MutationContext {
                connection,
                method: method::WRITING_VARIANT_REMOVE,
                params,
            },
            VariantChoice {
                file_id: &request.file_id,
                line_id: &request.line_id,
                variant_id: &request.variant_id,
                base_revision: &request.base_revision,
                remove: true,
            },
        )
    }

    fn change_variant_choice(
        &mut self,
        mutation: MutationContext<'_>,
        choice: VariantChoice<'_>,
    ) -> Result<Value> {
        if let Some(result) =
            self.repeated_mutation(mutation.connection, mutation.method, mutation.params)?
        {
            return Ok(result);
        }
        let author = self.file_author(mutation.connection)?;
        require_person(&author)?;
        let (file, mut record, line) =
            self.variant_context(choice.file_id, choice.base_revision, choice.line_id)?;
        let mut variants = take_variants(&mut record, &line)?;
        let selected = if choice.remove {
            variants.remove(choice.variant_id)?
        } else {
            variants.select(choice.variant_id)?
        }
        .to_owned();
        self.persist_variant_choice(
            mutation,
            VariantChoicePersistence {
                file,
                record,
                line,
                variants,
                selected,
                author,
            },
        )
    }

    fn persist_variant_choice(
        &mut self,
        mutation: MutationContext<'_>,
        persistence: VariantChoicePersistence,
    ) -> Result<Value> {
        let edit = line_edit(
            persistence.line.number,
            &persistence.line.text,
            persistence.selected,
        )?;
        let prepared = self.editor.prepare_edit(
            &persistence.file.file_id,
            persistence.file.revision,
            edit.as_slice(),
            persistence.author.clone(),
            self.now_ms(),
        )?;
        let next_file = prepared.file.clone();
        let next_record =
            advanced_record(persistence.record, &persistence.variants, self.now_ms())?;
        let result = variant_result(&next_file, &next_record, persistence.variants.public())?;
        let event = changed_event(&next_file, &next_record)?;
        let expected = next_record.sidecar_revision.saturating_sub(1);
        let committed = self.mutate(
            mutation.connection,
            mutation.method,
            mutation.params,
            |change| {
                change.save_editor_file(&next_file)?;
                change.advance_writing_sidecar(
                    expected,
                    &next_record,
                    &persistence.author,
                    next_record.updated_ms,
                )?;
                Ok((result, vec![(event::WRITING_CHANGED.into(), event)]))
            },
        )?;
        self.editor.commit_edit(prepared);
        self.track_language_file(&next_file);
        Ok(committed.result)
    }

    fn finish_variant_request(
        &mut self,
        connection: &str,
        params: &Value,
        request_id: String,
        delivered: bool,
    ) -> Result<Value> {
        let result = variant_request_result(request_id, delivered)?;
        Ok(self
            .mutate(connection, method::WRITING_VARIANT_REQUEST, params, |_| {
                Ok((result, Vec::new()))
            })?
            .result)
    }

    fn variant_context(
        &self,
        file_id: &str,
        revision: &str,
        line_id: &str,
    ) -> Result<(EditorFile, WritingDocumentRecord, VariantLine)> {
        let file = self.editor.stored(file_id)?.clone();
        require_revision(revision, file.revision)?;
        let record = self.writing_record(file_id)?;
        if record.kind != soyeht_protocol::WritingKind::Screenplay {
            return Err(errors::writing_kind_refused(
                "Line variants are available only in screenplay documents",
            ));
        }
        let source = file.draft_text.as_deref().unwrap_or(&file.disk_text);
        let line = variant_line(source, line_id)?;
        Ok((file, record, line))
    }

    fn variant_agent_is_live(&self, pane_id: &str, workspace_id: &str) -> Result<bool> {
        let Some(pane) = self.panes.get(pane_id) else {
            return Ok(false);
        };
        if pane.stored.pane.workspace_id != workspace_id {
            return Err(errors::unauthorized(
                "Variant agent belongs to another workspace",
            ));
        }
        Ok(self.agents.contains_key(pane_id)
            && !pane.stored.closed
            && pane.stored.pane.state != PaneState::Dead)
    }
}

#[derive(Clone)]
struct VariantLine {
    id: String,
    number: u32,
    text: String,
}

struct MutationContext<'a> {
    connection: &'a str,
    method: &'a str,
    params: &'a Value,
}

struct VariantChoice<'a> {
    file_id: &'a str,
    line_id: &'a str,
    variant_id: &'a str,
    base_revision: &'a str,
    remove: bool,
}

struct VariantPersistence {
    file: EditorFile,
    record: WritingDocumentRecord,
    variants: VariantSet,
    author: FileAuthor,
}

struct VariantChoicePersistence {
    file: EditorFile,
    record: WritingDocumentRecord,
    line: VariantLine,
    variants: VariantSet,
    selected: String,
    author: FileAuthor,
}

fn save_variant_sidecar(
    core: &mut Core,
    mutation: MutationContext<'_>,
    persistence: VariantPersistence,
) -> Result<Value> {
    let next = advanced_record(persistence.record, &persistence.variants, core.now_ms())?;
    let result = variant_result(&persistence.file, &next, persistence.variants.public())?;
    let event = changed_event(&persistence.file, &next)?;
    let expected = next.sidecar_revision.saturating_sub(1);
    Ok(core
        .mutate(
            mutation.connection,
            mutation.method,
            mutation.params,
            |change| {
                change.advance_writing_sidecar(
                    expected,
                    &next,
                    &persistence.author,
                    next.updated_ms,
                )?;
                Ok((result, vec![(event::WRITING_CHANGED.into(), event)]))
            },
        )?
        .result)
}

fn advanced_record(
    mut record: WritingDocumentRecord,
    variants: &VariantSet,
    now_ms: i64,
) -> Result<WritingDocumentRecord> {
    let public = variants.public();
    record
        .sidecar
        .line_variants
        .retain(|item| item.line_id != public.line_id);
    record.sidecar.line_variants.push(public);
    record.sidecar.line_variants.sort_by_key(|item| item.line);
    record.sidecar_revision = record
        .sidecar_revision
        .checked_add(1)
        .ok_or_else(|| errors::internal("Writing sidecar revision exhausted"))?;
    record.updated_ms = now_ms;
    Ok(record)
}

fn take_or_create_variants(
    record: &mut WritingDocumentRecord,
    line: &VariantLine,
    author: FileAuthor,
) -> Result<VariantSet> {
    match record
        .sidecar
        .line_variants
        .iter()
        .find(|item| item.line_id == line.id)
        .cloned()
    {
        Some(stored) => restore_variants(stored, line),
        None => VariantSet::new(
            line.id.clone(),
            line.number,
            line.text.clone(),
            author,
            Uuid::new_v4().to_string(),
        )
        .map_err(Into::into),
    }
}

fn take_variants(record: &mut WritingDocumentRecord, line: &VariantLine) -> Result<VariantSet> {
    let stored = record
        .sidecar
        .line_variants
        .iter()
        .find(|item| item.line_id == line.id)
        .cloned()
        .ok_or_else(|| errors::Error::Writing(soyeht_writing::Error::VariantNotFound))?;
    restore_variants(stored, line)
}

fn restore_variants(stored: WritingLineVariants, line: &VariantLine) -> Result<VariantSet> {
    let variants = VariantSet::restore(stored)?;
    if variants.public().line != line.number || variants.selected() != line.text {
        return Err(errors::stale_version(
            "Screenplay line changed outside its variant set",
        ));
    }
    Ok(variants)
}

fn variant_line(source: &str, line_id: &str) -> Result<VariantLine> {
    let screenplay = soyeht_writing::parse_screenplay(source)?;
    let element = screenplay
        .elements
        .into_iter()
        .find(|element| element.element_id == line_id)
        .ok_or_else(|| errors::Error::Writing(soyeht_writing::Error::VariantNotFound))?;
    let text = source
        .split('\n')
        .nth(element.start_line as usize)
        .ok_or_else(|| errors::Error::Writing(soyeht_writing::Error::VariantNotFound))?;
    Ok(VariantLine {
        id: line_id.to_owned(),
        number: element.start_line,
        text: text.to_owned(),
    })
}

fn validate_canonical(line_id: &str, line: u32, text: &str, author: FileAuthor) -> Result<()> {
    VariantSet::new(
        line_id.into(),
        line,
        text.into(),
        author,
        "canonical".into(),
    )?;
    Ok(())
}

fn line_edit(line: u32, current: &str, selected: String) -> Result<Vec<TextEdit>> {
    if current == selected {
        return Ok(Vec::new());
    }
    let column = u32::try_from(current.encode_utf16().count())
        .map_err(|_| errors::invalid("Screenplay line exceeds the position limit"))?;
    Ok(vec![TextEdit {
        range: TextRange {
            start: TextPosition { line, column: 0 },
            end: TextPosition { line, column },
        },
        text: selected,
    }])
}

fn variant_prompt(request: &WritingVariantRequestParams, line: &str) -> Result<String> {
    let instruction = request
        .instruction
        .as_deref()
        .unwrap_or("Write one distinct alternate take that preserves the scene intent.");
    let prompt = format!(
        "Create one screenplay line variant. File: {}. Line: {}. Revision: {}. Current line: {}. Instruction: {}. Read writing_structure if context is needed, then call writing_variant_add with the file, line, revision, and alternate text. Do not select the variant; the writer chooses.",
        request.file_id, request.line_id, request.base_revision, line, instruction
    );
    if prompt.len() > soyeht_protocol::limits::PROMPT_BYTES {
        return Err(errors::invalid("Variant request exceeds the prompt limit"));
    }
    Ok(prompt)
}

fn require_person(author: &FileAuthor) -> Result<()> {
    if !matches!(author.actor.role, Role::HumanLocal | Role::HumanDevice) {
        return Err(errors::forbidden_role(
            "Only a person may choose screenplay line variants",
        ));
    }
    Ok(())
}

fn require_agent(author: &FileAuthor) -> Result<()> {
    if !matches!(author.actor.role, Role::Agent | Role::Chief) {
        return Err(errors::forbidden_role(
            "Only a bound agent may add screenplay line variants",
        ));
    }
    Ok(())
}

pub(super) fn authorize_agent_workspace(
    core: &Core,
    author: &FileAuthor,
    workspace_id: &str,
) -> Result<()> {
    if matches!(author.actor.role, Role::HumanLocal | Role::HumanDevice) {
        return Ok(());
    }
    let pane_id = author
        .actor
        .pane_id
        .as_deref()
        .ok_or_else(|| errors::unauthorized("Variant agent pane is unavailable"))?;
    let pane = core
        .panes
        .get(pane_id)
        .ok_or_else(|| errors::unauthorized("Variant agent pane is unavailable"))?;
    if pane.stored.pane.workspace_id != workspace_id {
        return Err(errors::unauthorized(
            "Variant agent belongs to another workspace",
        ));
    }
    Ok(())
}

fn writer_author() -> FileAuthor {
    FileAuthor {
        actor: Actor {
            role: Role::HumanLocal,
            client: "writer".into(),
            device_id: None,
            pane_id: None,
        },
        name: "you".into(),
    }
}

fn variant_request_result(request_id: String, delivered: bool) -> Result<Value> {
    serde_json::to_value(WritingVariantRequestResult {
        request_id,
        delivered,
    })
    .map_err(|_| errors::internal("Variant request result cannot be encoded"))
}

fn variant_result(
    file: &EditorFile,
    record: &WritingDocumentRecord,
    variants: WritingLineVariants,
) -> Result<Value> {
    serde_json::to_value(WritingVariantResult {
        document: public_document(file, record)?,
        variants,
    })
    .map_err(|_| errors::internal("Variant result cannot be encoded"))
}

fn parameters<T: serde::de::DeserializeOwned>(params: &Value, name: &str) -> Result<T> {
    serde_json::from_value(params.clone())
        .map_err(|_| errors::invalid(format!("Invalid writing {name} parameters")))
}

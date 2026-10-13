use super::{
    documents::{changed_event, public_document, require_revision},
    variants::authorize_agent_workspace,
};
use crate::{core::Core, errors, errors::Result};
use serde_json::Value;
use soyeht_protocol::{
    BeatCard, FileAuthor, Role, WritingBeatDeleteParams, WritingBeatListParams,
    WritingBeatListResult, WritingBeatPutParams, WritingBeatResult, WritingKind, event, method,
};
use soyeht_store::{editor::EditorFile, writing::WritingDocumentRecord};
use soyeht_writing::BeatBoard;
use uuid::Uuid;

impl Core {
    pub(super) fn writing_beat_list(&self, connection: &str, params: &Value) -> Result<Value> {
        let request: WritingBeatListParams = parameters(params, "beat list")?;
        let author = self.file_author(connection)?;
        let context = self.beat_context(&request.file_id, &request.revision)?;
        authorize_agent_workspace(self, &author, &context.file.workspace_id)?;
        serde_json::to_value(WritingBeatListResult {
            beats: context.board.cards().to_vec(),
        })
        .map_err(|_| errors::internal("Writing beat list cannot be encoded"))
    }

    pub(super) fn writing_beat_put(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if let Some(result) =
            self.repeated_mutation(connection, method::WRITING_BEAT_PUT, params)?
        {
            return Ok(result);
        }
        let request: WritingBeatPutParams = parameters(params, "beat put")?;
        let author = self.file_author(connection)?;
        require_person(&author)?;
        let mut context = self.beat_context(&request.file_id, &request.base_revision)?;
        let beat_id = request
            .beat_id
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        context.board.put(
            BeatCard {
                beat_id: beat_id.clone(),
                title: request.title,
                body: request.body,
                category: request.category,
                order: request.order,
                scene_id: request.scene_id,
            },
            &context.scene_ids,
        )?;
        context.board.move_to(&beat_id, request.order as usize)?;
        let beat = context
            .board
            .cards()
            .iter()
            .find(|card| card.beat_id == beat_id)
            .cloned()
            .ok_or_else(|| errors::internal("Saved writing beat is unavailable"))?;
        self.persist_beat_change(
            connection,
            method::WRITING_BEAT_PUT,
            params,
            context,
            author,
            Some(beat),
        )
    }

    pub(super) fn writing_beat_delete(
        &mut self,
        connection: &str,
        params: &Value,
    ) -> Result<Value> {
        if let Some(result) =
            self.repeated_mutation(connection, method::WRITING_BEAT_DELETE, params)?
        {
            return Ok(result);
        }
        let request: WritingBeatDeleteParams = parameters(params, "beat delete")?;
        let author = self.file_author(connection)?;
        require_person(&author)?;
        let mut context = self.beat_context(&request.file_id, &request.base_revision)?;
        context.board.delete(&request.beat_id)?;
        self.persist_beat_change(
            connection,
            method::WRITING_BEAT_DELETE,
            params,
            context,
            author,
            None,
        )
    }

    fn beat_context(&self, file_id: &str, revision: &str) -> Result<BeatContext> {
        let file = self.editor.stored(file_id)?.clone();
        require_revision(revision, file.revision)?;
        let record = self.writing_record(file_id)?;
        if record.kind != WritingKind::Screenplay {
            return Err(errors::writing_kind_refused(
                "Beat boards are available only for screenplay documents",
            ));
        }
        let source = file.draft_text.as_deref().unwrap_or(&file.disk_text);
        let screenplay = soyeht_writing::parse_screenplay(source)?;
        let scene_ids = screenplay
            .scenes
            .into_iter()
            .map(|scene| scene.scene_id)
            .collect::<Vec<_>>();
        let board = BeatBoard::restore(record.sidecar.beat_cards.clone(), &scene_ids)?;
        Ok(BeatContext {
            file,
            record,
            scene_ids,
            board,
        })
    }

    fn persist_beat_change(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
        mut context: BeatContext,
        author: FileAuthor,
        beat: Option<BeatCard>,
    ) -> Result<Value> {
        context.record.sidecar.beat_cards = context.board.cards().to_vec();
        let expected = context.record.sidecar_revision;
        context.record.sidecar_revision = expected
            .checked_add(1)
            .ok_or_else(|| errors::internal("Writing sidecar revision exhausted"))?;
        context.record.updated_ms = self.now_ms();
        let result = beat_result(&context.file, &context.record, beat)?;
        let event = changed_event(&context.file, &context.record)?;
        Ok(self
            .mutate(connection, method, params, |change| {
                change.advance_writing_sidecar(
                    expected,
                    &context.record,
                    &author,
                    context.record.updated_ms,
                )?;
                Ok((result, vec![(event::WRITING_CHANGED.into(), event)]))
            })?
            .result)
    }
}

struct BeatContext {
    file: EditorFile,
    record: WritingDocumentRecord,
    scene_ids: Vec<String>,
    board: BeatBoard,
}

fn beat_result(
    file: &EditorFile,
    record: &WritingDocumentRecord,
    beat: Option<BeatCard>,
) -> Result<Value> {
    serde_json::to_value(WritingBeatResult {
        document: public_document(file, record)?,
        beat,
    })
    .map_err(|_| errors::internal("Writing beat result cannot be encoded"))
}

fn require_person(author: &FileAuthor) -> Result<()> {
    if !matches!(author.actor.role, Role::HumanLocal | Role::HumanDevice) {
        return Err(errors::forbidden_role(
            "Only a person may change the writing beat board",
        ));
    }
    Ok(())
}

fn parameters<T: serde::de::DeserializeOwned>(params: &Value, name: &str) -> Result<T> {
    serde_json::from_value(params.clone())
        .map_err(|_| errors::invalid(format!("Invalid writing {name} parameters")))
}

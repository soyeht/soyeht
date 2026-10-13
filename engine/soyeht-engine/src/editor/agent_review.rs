use super::persistence::{next_revision, stored_version};
use crate::{
    core::Core,
    errors::{self, Result},
    model,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Actor, DiffHunk, EditorReviewCommentParams, EditorReviewCommentResult, EditorReviewParams,
    EditorReviewResult, FileAuthor, PaneState, Role, TextPosition, TextRange, event, method,
};
use soyeht_store::{
    database::Change,
    editor::{EditorAgentReview, EditorFile, StoredFileVersion, document},
};
use std::path::Path;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct AgentWriteChange {
    file: EditorFile,
    version: StoredFileVersion,
    review: Option<EditorAgentReview>,
}

impl AgentWriteChange {
    pub(crate) fn persist(&self, change: &Change<'_>) -> soyeht_store::database::Result<()> {
        change.save_editor_file(&self.file)?;
        change.record_file_version(&self.version)?;
        if let Some(review) = &self.review {
            change.save_editor_agent_review(review)?;
        } else {
            change.clear_editor_agent_review(&self.file.file_id)?;
        }
        Ok(())
    }
}

pub(crate) struct PreparedAgentWrite {
    pub(crate) current_disk: String,
    pub(crate) change: AgentWriteChange,
    pub(crate) events: Vec<(String, Value)>,
}

impl Core {
    pub(crate) fn prepare_agent_write(
        &mut self,
        pane_id: &str,
        path: &Path,
        content: &str,
        recovering: bool,
    ) -> Result<Option<PreparedAgentWrite>> {
        let pane = self
            .panes
            .get(pane_id)
            .ok_or_else(|| errors::not_found("Agent pane not found"))?;
        let workspace = model::workspace(&self.store, &pane.stored.pane.workspace_id)?;
        let root = Path::new(&workspace.path);
        let Some(resolved) = self.editor.resolve_absolute_if_contained(root, path)? else {
            return Ok(None);
        };
        let Some(file) = self
            .store
            .editor_file_for_path(&workspace.workspace_id, &resolved.relative)
            .map_err(errors::storage)?
        else {
            return Ok(None);
        };
        if !self.editor.is_open(&file.file_id) {
            return Ok(None);
        }
        self.agent_write_change(pane_id, file, content, recovering)
    }

    fn agent_write_change(
        &self,
        pane_id: &str,
        mut file: EditorFile,
        content: &str,
        recovering: bool,
    ) -> Result<Option<PreparedAgentWrite>> {
        if file.draft_text.is_some() || file.conflict_text.is_some() {
            return Err(errors::stale_version(
                "Agent write requires a clean editor file",
            ));
        }
        let workspace = model::workspace(&self.store, &file.workspace_id)?;
        let actual = self
            .editor
            .current_disk(Path::new(&workspace.path), &file)?;
        if actual != file.disk_text && !(recovering && actual == content) {
            return Err(errors::stale_version(
                "Agent write found a concurrent disk change",
            ));
        }
        if actual == content && file.disk_text == content {
            return Ok(None);
        }
        let author = self.agent_file_author(pane_id)?;
        let previous = self
            .store
            .editor_agent_review(&file.file_id)
            .map_err(errors::storage)?;
        let baseline = review_baseline(previous.as_ref(), &file, pane_id)?;
        file.revision = next_revision(file.revision)?;
        file.disk_revision = next_revision(file.disk_revision)?;
        file.disk_text = content.into();
        file.draft_text = None;
        file.draft_author = None;
        file.conflict_text = None;
        file.updated_ms = self.now_ms();
        let diff = self.vcs.review(content, &baseline)?;
        let review = (!diff.hunks.is_empty()).then(|| EditorAgentReview {
            file_id: file.file_id.clone(),
            revision: file.revision,
            pane_id: pane_id.into(),
            author: author.clone(),
            baseline_text: baseline,
            updated_ms: self.now_ms(),
        });
        let version = stored_version(&file, content.into(), author.clone(), self.timestamp());
        let events = agent_write_events(&file, pane_id, &author, diff.hunks)?;
        Ok(Some(PreparedAgentWrite {
            current_disk: actual,
            change: AgentWriteChange {
                file,
                version,
                review,
            },
            events,
        }))
    }

    pub(crate) fn install_agent_write(&mut self, write: AgentWriteChange) {
        self.editor.replace(write.file.clone(), self.now_ms());
        self.track_language_file(&write.file);
    }

    pub(crate) fn write_prepared_agent_file(
        &self,
        write: &AgentWriteChange,
        expected: &str,
        content: &str,
    ) -> Result<()> {
        let workspace = model::workspace(&self.store, &write.file.workspace_id)?;
        if !self
            .editor
            .save_disk(Path::new(&workspace.path), &write.file, expected, content)?
        {
            return Err(errors::stale_version(
                "Agent write lost a concurrent disk race",
            ));
        }
        Ok(())
    }

    pub fn editor_review(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        let request: EditorReviewParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid editor review parameters"))?;
        let base = parse_revision(&request.base_revision)?;
        let current = self.editor.stored(&request.file_id)?.clone();
        let review = self
            .store
            .editor_agent_review(&request.file_id)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::not_found("Agent review not found"))?;
        if current.revision != base || review.revision != base {
            return Err(errors::stale_version("Agent review revision changed"));
        }
        if current.draft_text.is_some() || current.conflict_text.is_some() {
            return Err(errors::stale_version("Agent review file is not clean"));
        }
        let resolved =
            self.resolve_review(method, connection, current, review, &request.hunk_id)?;
        let result = serde_json::to_value(EditorReviewResult {
            file: document(resolved.file.clone()).map_err(errors::storage)?,
            hunk_id: request.hunk_id,
        })
        .map_err(|_| errors::internal("Editor review result serialization failed"))?;
        let saved = resolved.clone();
        let mutation = self.mutate(connection, method, params, |change| {
            saved.persist(change)?;
            Ok((result, saved.events.clone()))
        })?;
        if !mutation.repeated {
            self.editor.replace(resolved.file.clone(), self.now_ms());
            self.track_language_file(&resolved.file);
        }
        Ok(mutation.result)
    }

    pub fn editor_review_comment(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if let Some(result) =
            self.repeated_mutation(connection, method::EDITOR_REVIEW_COMMENT, params)?
        {
            return Ok(result);
        }
        let request: EditorReviewCommentParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid editor review comment parameters"))?;
        let (pane_id, text) = self.review_comment_context(connection, &request)?;
        let comment_id = Uuid::new_v4().to_string();
        if !self.review_author_is_live(&pane_id) {
            return self.finish_undelivered_comment(connection, params, comment_id);
        }
        let task_feedback = self.prepare_editor_change_request(&pane_id, &comment_id, &text)?;
        let task_id = task_feedback
            .as_ref()
            .map(|feedback| feedback.task_id().to_owned());
        let mut state = self.agent_state(&pane_id)?;
        let prompt = state.enqueue_feedback(
            format!("task-feedback:editor:{comment_id}"),
            text,
            self.timestamp(),
        )?;
        let mut events = task_feedback
            .as_ref()
            .map_or_else(Vec::new, |feedback| feedback.events());
        events.push((
            event::PANE_QUEUED.into(),
            json!({"paneId":pane_id,"n":state.queue.len(),"promptIds":[prompt.prompt_id],"draft":false}),
        ));
        let result = review_comment_result(comment_id, task_id, true)?;
        let saved = self.panes[&pane_id].stored.clone();
        let changes = crate::agents::Changes {
            events,
            task_editor_feedback: task_feedback,
            ..crate::agents::Changes::default()
        };
        Ok(self
            .commit_agent(
                saved,
                state,
                changes,
                Some((connection, method::EDITOR_REVIEW_COMMENT, params)),
                result,
            )?
            .result)
    }

    fn review_comment_context(
        &self,
        connection: &str,
        request: &EditorReviewCommentParams,
    ) -> Result<(String, String)> {
        let author = self.file_author(connection)?;
        if !matches!(author.actor.role, Role::HumanLocal | Role::HumanDevice) {
            return Err(errors::forbidden_role(
                "Only a person may request an editor review change",
            ));
        }
        let body = request.body.trim();
        if body.is_empty() {
            return Err(errors::invalid("Review comment must not be empty"));
        }
        let file = self.editor.stored(&request.file_id)?;
        let review = self
            .store
            .editor_agent_review(&request.file_id)
            .map_err(errors::storage)?
            .ok_or_else(|| errors::not_found("Agent review not found"))?;
        if review.revision != file.revision {
            return Err(errors::stale_version("Agent review revision changed"));
        }
        if file.draft_text.is_some() || file.conflict_text.is_some() {
            return Err(errors::stale_version("Agent review file is not clean"));
        }
        validate_comment_range(&file.disk_text, &request.range)?;
        let location = line_location(&file.relative_path, &request.range.start)?;
        let text = format!("Change request at {location}\n\n{body}");
        if text.len() > soyeht_protocol::limits::PROMPT_BYTES {
            return Err(errors::invalid(
                "Review comment and location exceed the prompt limit",
            ));
        }
        Ok((review.pane_id, text))
    }

    fn review_author_is_live(&self, pane_id: &str) -> bool {
        self.agents.contains_key(pane_id)
            && self.panes.get(pane_id).is_some_and(|pane| {
                !pane.stored.closed && pane.stored.pane.state != PaneState::Dead
            })
    }

    fn finish_undelivered_comment(
        &mut self,
        connection: &str,
        params: &Value,
        comment_id: String,
    ) -> Result<Value> {
        let result = review_comment_result(comment_id, None, false)?;
        Ok(self
            .mutate(connection, method::EDITOR_REVIEW_COMMENT, params, |_| {
                Ok((result, Vec::new()))
            })?
            .result)
    }

    fn resolve_review(
        &self,
        method: &str,
        connection: &str,
        mut file: EditorFile,
        mut review: EditorAgentReview,
        hunk_id: &str,
    ) -> Result<ResolvedReview> {
        let current = file.disk_text.clone();
        if method == method::EDITOR_REVIEW_ACCEPT {
            review.baseline_text =
                self.vcs
                    .accept_hunk(&current, &review.baseline_text, hunk_id)?;
        } else {
            let next = self
                .vcs
                .reject_hunk(&current, &review.baseline_text, hunk_id)?;
            file.revision = next_revision(file.revision)?;
            file.draft_text = Some(next);
            file.draft_author = Some(self.file_author(connection)?);
            file.updated_ms = self.now_ms();
            review.revision = file.revision;
        }
        let next_text = file.draft_text.as_deref().unwrap_or(&file.disk_text);
        let diff = self.vcs.review(next_text, &review.baseline_text)?;
        let pending = !diff.hunks.is_empty();
        let events = review_events(&file, &review, diff.hunks)?;
        Ok(ResolvedReview {
            file,
            review: pending.then_some(review),
            events,
        })
    }

    fn agent_file_author(&self, pane_id: &str) -> Result<FileAuthor> {
        let pane = self
            .panes
            .get(pane_id)
            .ok_or_else(|| errors::not_found("Agent pane not found"))?;
        let role = self.agent_state(pane_id)?.mcp_role;
        if !matches!(role, Role::Agent | Role::Chief) {
            return Err(errors::internal("Agent pane has an invalid author role"));
        }
        Ok(FileAuthor {
            actor: Actor {
                role,
                client: "agent-session".into(),
                device_id: None,
                pane_id: Some(pane_id.into()),
            },
            name: pane.stored.pane.name.clone(),
        })
    }
}

#[derive(Clone)]
struct ResolvedReview {
    file: EditorFile,
    review: Option<EditorAgentReview>,
    events: Vec<(String, Value)>,
}

impl ResolvedReview {
    fn persist(&self, change: &Change<'_>) -> soyeht_store::database::Result<()> {
        change.save_editor_file(&self.file)?;
        if let Some(review) = &self.review {
            change.save_editor_agent_review(review)?;
        } else {
            change.clear_editor_agent_review(&self.file.file_id)?;
        }
        Ok(())
    }
}

fn review_baseline(
    review: Option<&EditorAgentReview>,
    file: &EditorFile,
    pane_id: &str,
) -> Result<String> {
    let Some(review) = review else {
        return Ok(file.disk_text.clone());
    };
    if review.pane_id != pane_id || review.revision != file.revision {
        return Err(errors::stale_version(
            "Another editor review must be resolved first",
        ));
    }
    Ok(review.baseline_text.clone())
}

fn agent_write_events(
    file: &EditorFile,
    pane_id: &str,
    author: &FileAuthor,
    hunks: Vec<soyeht_vcs::Hunk>,
) -> Result<Vec<(String, Value)>> {
    let wire = document(file.clone()).map_err(errors::storage)?;
    Ok(vec![
        (event::FILE_SAVED.into(), json!({"file":wire,"by":author})),
        (
            event::EDITOR_REVIEW_REQUESTED.into(),
            review_event(file, pane_id, hunks, author),
        ),
        (
            event::EDITOR_AGENT_CURSOR.into(),
            json!({"fileId":file.file_id,"paneId":pane_id,"name":author.name,"position":end_position(&file.disk_text)?}),
        ),
    ])
}

fn review_events(
    file: &EditorFile,
    review: &EditorAgentReview,
    hunks: Vec<soyeht_vcs::Hunk>,
) -> Result<Vec<(String, Value)>> {
    Ok(vec![(
        event::EDITOR_REVIEW_REQUESTED.into(),
        review_event(file, &review.pane_id, hunks, &review.author),
    )])
}

fn review_event(
    file: &EditorFile,
    pane_id: &str,
    hunks: Vec<soyeht_vcs::Hunk>,
    author: &FileAuthor,
) -> Value {
    let hunks: Vec<DiffHunk> = hunks
        .into_iter()
        .map(|hunk| crate::vcs::wire_hunk(hunk, Some(author)))
        .collect();
    json!({"fileId":file.file_id,"revision":file.revision.to_string(),"paneId":pane_id,"hunks":hunks})
}

fn end_position(text: &str) -> Result<TextPosition> {
    let line = u32::try_from(text.bytes().filter(|byte| *byte == b'\n').count())
        .map_err(|_| errors::internal("Agent cursor line exceeds the protocol"))?;
    let column = u32::try_from(
        text.rsplit('\n')
            .next()
            .unwrap_or_default()
            .encode_utf16()
            .count(),
    )
    .map_err(|_| errors::internal("Agent cursor column exceeds the protocol"))?;
    Ok(TextPosition { line, column })
}

fn parse_revision(value: &str) -> Result<u64> {
    value
        .parse()
        .map_err(|_| errors::invalid("Invalid editor review revision"))
}

fn validate_comment_range(text: &str, range: &TextRange) -> Result<()> {
    let buffer = soyeht_editor::TextBuffer::new(text);
    let start = buffer.byte_at(&range.start)?;
    let end = buffer.byte_at(&range.end)?;
    if start > end {
        return Err(errors::invalid("Review comment range is reversed"));
    }
    Ok(())
}

fn line_location(path: &str, position: &TextPosition) -> Result<String> {
    let line = position
        .line
        .checked_add(1)
        .ok_or_else(|| errors::invalid("Review comment line exceeds its range"))?;
    let column = position
        .column
        .checked_add(1)
        .ok_or_else(|| errors::invalid("Review comment column exceeds its range"))?;
    Ok(format!("{path}:{line}:{column}"))
}

fn review_comment_result(
    comment_id: String,
    task_id: Option<String>,
    delivered: bool,
) -> Result<Value> {
    serde_json::to_value(EditorReviewCommentResult {
        comment_id,
        task_id,
        delivered,
    })
    .map_err(|_| errors::internal("Editor review comment result serialization failed"))
}

//! Pure CI and review feedback decisions.

use crate::{
    agents::Changes,
    core::Core,
    errors::{self, Result},
    ports::StorePort,
    tasks::{PhraseFacts, phrase, review_badge, revision::RevisionSnapshot},
};
use serde_json::{Value, json};
use soyeht_protocol::{
    AttentionItem, Comment, PaneState, Task, TaskCheckState, TaskPullRequest, TaskReviewBadge,
    TaskReviewState, event,
};
use soyeht_store::{agents, database::Change, tasks::FeedbackState};
use uuid::Uuid;

const DELIVERY_LIMIT: u32 = 3;
const FEEDBACK_TEXT_LIMIT: usize = 64 * 1024;
const REVISION_POLL_INTERVAL_MS: i64 = 30_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedbackPaneFacts {
    pub owner_alive: bool,
    pub permission_pending: bool,
    pub human_draft_active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedbackMessage {
    pub prompt_id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeedbackDecision {
    pub task: Task,
    pub state: FeedbackState,
    pub message: Option<FeedbackMessage>,
    pub needs_attention: bool,
}

#[derive(Clone)]
pub(crate) struct TaskFeedbackChange {
    expected_version: u32,
    task: Task,
    state: FeedbackState,
}

impl TaskFeedbackChange {
    pub(crate) fn persist(&self, change: &Change<'_>) -> soyeht_store::Result<()> {
        if self.task.version != self.expected_version {
            change.replace_task(&self.task, self.expected_version)?;
        }
        change.save_feedback(&self.task.workspace_id, &self.task.task_id, &self.state)
    }
}

#[derive(Clone)]
pub(crate) struct TaskEditorFeedbackChange {
    expected_version: u32,
    task: Task,
    comment: Comment,
}

impl TaskEditorFeedbackChange {
    pub(crate) fn persist(&self, change: &Change<'_>) -> soyeht_store::Result<()> {
        change.replace_task(&self.task, self.expected_version)?;
        change.insert_comment(&self.task.workspace_id, &self.task.task_id, &self.comment)
    }

    pub(crate) fn task_id(&self) -> &str {
        &self.task.task_id
    }

    pub(crate) fn events(&self) -> Vec<(String, Value)> {
        vec![
            (
                event::TASK_COMMENTED.into(),
                json!({"workspaceId":self.task.workspace_id,"taskId":self.task.task_id,"comment":self.comment}),
            ),
            (
                event::TASK_UPDATED.into(),
                json!({"workspaceId":self.task.workspace_id,"taskId":self.task.task_id,"version":self.task.version}),
            ),
            (
                event::TASK_PHRASE_CHANGED.into(),
                json!({"workspaceId":self.task.workspace_id,"taskId":self.task.task_id,"phrase":self.task.phrase}),
            ),
        ]
    }
}

impl Core {
    pub(crate) fn prepare_editor_change_request(
        &self,
        pane_id: &str,
        comment_id: &str,
        text: &str,
    ) -> Result<Option<TaskEditorFeedbackChange>> {
        let Some(current) = self
            .store
            .active_task_for_pane(pane_id)
            .map_err(errors::storage)?
        else {
            return Ok(None);
        };
        let mut task = current.clone();
        task.version = task
            .version
            .checked_add(1)
            .ok_or_else(|| errors::internal("Task version exhausted"))?;
        task.phrase = "review requested changes".into();
        task.review_badge = Some(TaskReviewBadge::SystemTurn);
        let comment = Comment {
            comment_id: comment_id.into(),
            task_id: task.task_id.clone(),
            author: "you".into(),
            text: text.into(),
            at: self.timestamp(),
        };
        Ok(Some(TaskEditorFeedbackChange {
            expected_version: current.version,
            task,
            comment,
        }))
    }

    pub(crate) fn advance_task_feedback(&mut self) -> Result<()> {
        let now = self.now_ms();
        let tasks = self
            .store
            .tasks_with_pull_requests()
            .map_err(errors::storage)?;
        let mut first_error = None;
        for task in tasks {
            let key = (task.workspace_id.clone(), task.task_id.clone());
            if self
                .task_revision_due
                .get(&key)
                .is_some_and(|due| *due > now)
            {
                continue;
            }
            self.task_revision_due
                .insert(key, now.saturating_add(REVISION_POLL_INTERVAL_MS));
            if let Err(error) = self.inspect_task_revision(task)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Decides and delivers one task's feedback for the revision git read.
    pub(super) fn feedback(&mut self, task: Task, revision: &RevisionSnapshot) -> Result<()> {
        let prior = self
            .store
            .feedback_state(&task.workspace_id, &task.task_id)
            .map_err(errors::storage)?;
        let pane = task.owner_pane_id.clone();
        let facts = self.feedback_pane_facts(pane.as_deref())?;
        let decision = evaluate(&task, revision, prior.as_ref(), &facts)?;
        let attention = self.feedback_attention(&decision, pane.as_deref())?;
        if let Some(pane) = pane
            .as_ref()
            .filter(|pane| self.agents.contains_key(pane.as_str()))
            .cloned()
        {
            return self.commit_feedback_to_pane(pane, task, decision, attention);
        }
        self.commit_feedback_without_pane(task, decision, attention)
    }

    fn feedback_pane_facts(&self, pane: Option<&str>) -> Result<FeedbackPaneFacts> {
        let Some(pane) = pane else {
            return Ok(FeedbackPaneFacts::default());
        };
        let owner_alive = self.panes.get(pane).is_some_and(|running| {
            !running.stored.closed && running.stored.pane.state != PaneState::Dead
        });
        let permission_pending = !self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?
            .is_empty();
        let human_draft_active = self
            .agents
            .get(pane)
            .is_some_and(|running| running.state.draft_until_ms > self.now_ms());
        Ok(FeedbackPaneFacts {
            owner_alive,
            permission_pending,
            human_draft_active,
        })
    }

    fn feedback_attention(
        &self,
        decision: &FeedbackDecision,
        pane: Option<&str>,
    ) -> Result<AttentionChange> {
        let id = format!("task:{}:feedback", decision.task.task_id);
        let prior = agents::attention(&self.store, &decision.task.workspace_id)
            .map_err(errors::storage)?
            .into_iter()
            .find(|item| item.attention_id == id);
        let next = if decision.needs_attention {
            pane.map(|pane| AttentionItem {
                attention_id: id,
                pane_id: pane.into(),
                request_id: None,
                message: bounded_owned(
                    &format!("{}: {}", decision.task.task_id, decision.task.phrase),
                    1024,
                ),
                created_at: prior
                    .as_ref()
                    .map_or_else(|| self.timestamp(), |item| item.created_at.clone()),
            })
        } else {
            None
        };
        Ok(AttentionChange { prior, next })
    }

    fn commit_feedback_to_pane(
        &mut self,
        pane: String,
        current: Task,
        decision: FeedbackDecision,
        attention: AttentionChange,
    ) -> Result<()> {
        let mut state = self.agent_state(&pane)?;
        let prompt = decision
            .message
            .as_ref()
            .map(|message| {
                state.enqueue_feedback(
                    message.prompt_id.clone(),
                    message.text.clone(),
                    self.timestamp(),
                )
            })
            .transpose()?;
        let events = feedback_events(
            &current,
            &decision.task,
            prompt.as_ref().map(|prompt| (prompt, state.queue.len())),
            &attention,
        )?;
        let mut changes = Changes {
            events,
            task_feedback: Some(feedback_change(&current, &decision)),
            ..Changes::default()
        };
        apply_attention_change(&mut changes, attention);
        let saved = self.panes[&pane].stored.clone();
        self.commit_agent(saved, state, changes, None, json!({}))?;
        Ok(())
    }

    fn commit_feedback_without_pane(
        &mut self,
        current: Task,
        decision: FeedbackDecision,
        attention: AttentionChange,
    ) -> Result<()> {
        let key = Uuid::new_v4().to_string();
        let events = feedback_events(&current, &decision.task, None, &attention)?;
        let update = feedback_change(&current, &decision);
        let workspace = current.workspace_id;
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-feedback",
            &key,
            "task-feedback",
            |change| {
                update.persist(change)?;
                persist_attention_change(change, &workspace, &attention)?;
                let mut committed = events;
                if attention.next != attention.prior {
                    let items = agents::attention_in_change(change, &workspace)?;
                    committed.push((
                        event::ATTENTION_CHANGED.into(),
                        json!({"workspaceId":workspace,"count":items.len(),"items":items}),
                    ));
                }
                Ok((json!({}), committed))
            },
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)
    }
}

#[derive(Clone)]
struct AttentionChange {
    prior: Option<AttentionItem>,
    next: Option<AttentionItem>,
}

fn feedback_change(current: &Task, decision: &FeedbackDecision) -> TaskFeedbackChange {
    TaskFeedbackChange {
        expected_version: current.version,
        task: decision.task.clone(),
        state: decision.state.clone(),
    }
}

fn apply_attention_change(changes: &mut Changes, attention: AttentionChange) {
    if attention.next != attention.prior {
        if let Some(item) = attention.next {
            changes.attention.push(item);
        } else if let Some(item) = attention.prior {
            changes.remove_attention.push(item.attention_id);
        }
    }
}

fn persist_attention_change(
    change: &Change<'_>,
    workspace: &str,
    attention: &AttentionChange,
) -> soyeht_store::Result<()> {
    if attention.next == attention.prior {
        return Ok(());
    }
    if let Some(item) = &attention.next {
        agents::save_attention(change, workspace, item)?;
    } else if let Some(item) = &attention.prior {
        change.remove_attention(&item.pane_id, &item.attention_id)?;
    }
    Ok(())
}

fn feedback_events(
    current: &Task,
    updated: &Task,
    prompt: Option<(&soyeht_protocol::PromptItem, usize)>,
    attention: &AttentionChange,
) -> Result<Vec<(String, Value)>> {
    let mut events = Vec::new();
    if current != updated {
        events.push(super::events::updated(updated));
        if current.phrase != updated.phrase {
            events.push(super::events::phrase_changed(updated));
        }
    }
    if let Some((prompt, count)) = prompt {
        events.push(crate::model::pane_queued(
            updated
                .owner_pane_id
                .as_deref()
                .ok_or_else(|| errors::internal("Queued feedback has no owner pane"))?,
            count,
            vec![prompt.prompt_id.clone()],
            false,
        )?);
    }
    if attention.next.is_some() && attention.next != attention.prior {
        events.push((
            event::TASK_ATTENTION.into(),
            json!({"workspaceId":updated.workspace_id,"taskId":updated.task_id,"n":1}),
        ));
    }
    Ok(events)
}

pub fn evaluate(
    task: &Task,
    revision: &RevisionSnapshot,
    previous: Option<&FeedbackState>,
    pane: &FeedbackPaneFacts,
) -> Result<FeedbackDecision> {
    validate_revision(revision)?;
    let task = task_with_revision(task, revision, pane.owner_alive)?;
    if !needs_feedback(revision) {
        return Ok(FeedbackDecision {
            task,
            state: rearmed(previous, revision),
            message: None,
            needs_attention: false,
        });
    }
    let mut state = previous.cloned().unwrap_or_else(|| FeedbackState {
        fingerprint: String::new(),
        delivery_count: 0,
        armed: true,
    });
    let unseen = state.fingerprint != revision.fingerprint;
    let eligible = state.delivery_count < DELIVERY_LIMIT && (state.armed || unseen);
    let deferred = pane.permission_pending || pane.human_draft_active;
    if !eligible || deferred || !pane.owner_alive {
        return Ok(FeedbackDecision {
            task,
            state,
            message: None,
            needs_attention: !pane.owner_alive,
        });
    }
    state.delivery_count += 1;
    state.fingerprint.clone_from(&revision.fingerprint);
    state.armed = false;
    Ok(FeedbackDecision {
        message: Some(message(&task, revision, state.delivery_count)),
        task,
        state,
        needs_attention: false,
    })
}

fn validate_revision(revision: &RevisionSnapshot) -> Result<()> {
    if revision.head.trim().is_empty() || revision.fingerprint.trim().is_empty() {
        return Err(errors::invalid(
            "Revision head and fingerprint are required",
        ));
    }
    if needs_feedback(revision) && revision.summary.trim().is_empty() {
        return Err(errors::invalid("Failed revision summary is required"));
    }
    Ok(())
}

fn task_with_revision(task: &Task, revision: &RevisionSnapshot, owner_alive: bool) -> Result<Task> {
    let mut updated = task.clone();
    let url = task
        .pr
        .as_ref()
        .map(|pr| pr.url.clone())
        .ok_or_else(|| errors::invalid("Task has no pull request"))?;
    let pr = TaskPullRequest {
        url,
        check_state: revision.check_state.clone(),
        review_state: revision.review_state.clone(),
        head: Some(revision.head.clone()),
    };
    updated.pr = Some(pr);
    updated.review_badge = review_badge(&updated, owner_alive);
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    if updated != *task {
        updated.version = task
            .version
            .checked_add(1)
            .ok_or_else(|| errors::internal("Task version exhausted"))?;
    }
    Ok(updated)
}

fn needs_feedback(revision: &RevisionSnapshot) -> bool {
    matches!(
        revision.check_state,
        TaskCheckState::Red | TaskCheckState::Conflict
    ) || revision.review_state == TaskReviewState::ChangesRequested
}

fn rearmed(previous: Option<&FeedbackState>, revision: &RevisionSnapshot) -> FeedbackState {
    let mut state = previous.cloned().unwrap_or_else(|| FeedbackState {
        fingerprint: revision.fingerprint.clone(),
        delivery_count: 0,
        armed: true,
    });
    state.fingerprint.clone_from(&revision.fingerprint);
    state.armed = revision.check_state == TaskCheckState::Green;
    state
}

fn message(task: &Task, revision: &RevisionSnapshot, delivery: u32) -> FeedbackMessage {
    let text = bounded_owned(
        &format!(
            "{}\n\nLog tail:\n{}",
            revision.summary,
            bounded(&revision.log_tail, FEEDBACK_TEXT_LIMIT)
        ),
        soyeht_protocol::limits::PROMPT_BYTES,
    );
    FeedbackMessage {
        prompt_id: format!("task-feedback:{}:{delivery}", task.task_id),
        text,
    }
}

fn bounded_owned(text: &str, limit: usize) -> String {
    bounded(text, limit).to_owned()
}

fn bounded(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests;

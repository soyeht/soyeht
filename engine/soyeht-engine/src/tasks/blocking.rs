use super::{PhraseFacts, events, next_version, phrase};
use crate::{
    agents::{Changes, state::State},
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Attempt, AttemptResult, AttentionItem, Blocking, BlockingKind, Comment, PaneState,
    PromptOrigin, Task, TaskMove, TaskState, limits,
};
use soyeht_store::database::{Change, Result as StoreResult};
use uuid::Uuid;

pub(crate) const RETRY_DELAY_MS: i64 = 30_000;

#[derive(Debug, PartialEq)]
enum BlockingAction {
    None,
    DependencyCompleted(Task),
    DependencyCancelled(Task),
    Retry(Task),
    NeedsInformation(Task),
}

pub(crate) struct TaskBlockingChange {
    expected_version: u32,
    task: Task,
    attempt: Option<Attempt>,
    movement: Option<TaskMove>,
    comment: Option<Comment>,
    next_retry_ms: Option<i64>,
}

impl TaskBlockingChange {
    pub(crate) fn persist(&self, change: &Change<'_>) -> StoreResult<()> {
        change.replace_task(&self.task, self.expected_version)?;
        if let Some(attempt) = &self.attempt {
            change.replace_attempt(&self.task.workspace_id, &self.task.task_id, attempt)?;
        }
        if let Some(movement) = &self.movement {
            change.insert_move(&self.task.workspace_id, &self.task.task_id, movement)?;
        }
        if let Some(comment) = &self.comment {
            change.insert_comment(&self.task.workspace_id, &self.task.task_id, comment)?;
        }
        if let Some(due) = self.next_retry_ms {
            change.schedule_task_retry(&self.task.workspace_id, &self.task.task_id, due)
        } else {
            change.clear_task_retry(&self.task.workspace_id, &self.task.task_id)
        }
    }

    fn events(&self) -> Vec<(String, Value)> {
        let task = &self.task;
        let mut emitted = events::revised(task, true);
        if let Some(movement) = &self.movement {
            emitted.push(events::moved(task, movement));
        }
        if let Some(comment) = &self.comment {
            emitted.push(events::commented(task, comment));
        }
        emitted
    }
}

fn dependency_action(task: &Task, dependency: &Task) -> Result<BlockingAction> {
    match dependency.state {
        TaskState::Completed => {
            let mut updated = task.clone();
            updated.blocking = None;
            updated.state = TaskState::Todo;
            updated.owner_pane_id = None;
            updated.review_badge = None;
            updated.version = next_version(task)?;
            updated.phrase = phrase(&updated, &PhraseFacts::default());
            Ok(BlockingAction::DependencyCompleted(updated))
        }
        TaskState::Cancelled
            if task
                .blocking
                .as_ref()
                .is_some_and(|blocking| blocking.reason != "dependency cancelled") =>
        {
            let mut updated = task.clone();
            let blocking = errors::invariant(
                updated.blocking.as_mut(),
                "Blocked task lost its blocking value",
            )?;
            blocking.reason = "dependency cancelled".into();
            updated.version = next_version(task)?;
            updated.phrase = phrase(&updated, &PhraseFacts::default());
            Ok(BlockingAction::DependencyCancelled(updated))
        }
        _ => Ok(BlockingAction::None),
    }
}

fn external_action(task: &Task, owner_alive: bool) -> Result<BlockingAction> {
    let blocking = errors::invariant(
        task.blocking.as_ref(),
        "Scheduled task lost its blocking value",
    )?;
    let count = blocking.retry_count.unwrap_or(0);
    let mut updated = task.clone();
    if owner_alive && count < 2 {
        let value = errors::invariant(
            updated.blocking.as_mut(),
            "Scheduled task lost its blocking value",
        )?;
        value.retry_count = Some(count + 1);
        updated.version = next_version(task)?;
        return Ok(BlockingAction::Retry(updated));
    }
    updated.blocking = Some(Blocking {
        kind: BlockingKind::MissingInformation,
        reason: blocking.reason.clone(),
        dependency_task_id: None,
        retry_count: None,
    });
    updated.version = next_version(task)?;
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    Ok(BlockingAction::NeedsInformation(updated))
}

pub(crate) fn attention(task: &Task, pane: &str, timestamp: String) -> AttentionItem {
    AttentionItem {
        attention_id: format!("task:{}:blocking", task.task_id),
        pane_id: pane.into(),
        request_id: None,
        message: format!("{}: {}", task.task_id, task.phrase),
        created_at: timestamp,
    }
}

impl Core {
    pub(crate) fn advance_task_blocking(&mut self) -> Result<()> {
        self.advance_dependency_blocks()?;
        let due = self
            .store
            .due_task_retries(self.now_ms())
            .map_err(errors::storage)?;
        for task in due {
            self.advance_external_block(task)?;
        }
        Ok(())
    }

    fn advance_dependency_blocks(&mut self) -> Result<()> {
        // A dependency block ends only when a task row changes.
        let Some(seen) = self.store.begin_poll("dependency_blocks", self.now_ms()) else {
            return Ok(());
        };
        let blocked = self.store.blocked_tasks().map_err(errors::storage)?;
        for task in blocked {
            let Some(blocking) = task.blocking.as_ref() else {
                continue;
            };
            if blocking.kind != BlockingKind::Dependency {
                continue;
            }
            let dependency_id = errors::invariant(
                blocking.dependency_task_id.as_deref(),
                "Dependency block has no task id",
            )?;
            let dependency = self
                .store
                .task(&task.workspace_id, dependency_id)
                .map_err(errors::storage)?
                .ok_or_else(|| errors::internal("Dependency task disappeared"))?;
            match dependency_action(&task, &dependency)? {
                BlockingAction::DependencyCompleted(updated) => {
                    self.finish_dependency_block(task, updated)?;
                }
                BlockingAction::DependencyCancelled(updated) => {
                    self.persist_block_change(task, updated, None, true)?;
                }
                _ => {}
            }
        }
        self.store.rest("dependency_blocks", seen, i64::MAX);
        Ok(())
    }

    fn finish_dependency_block(&mut self, task: Task, updated: Task) -> Result<()> {
        let at = self.timestamp();
        let mut attempt = task
            .owner_pane_id
            .as_ref()
            .map(|_| self.open_attempt(&task))
            .transpose()?;
        if let Some(value) = &mut attempt {
            value.end = Some(at.clone());
            value.result = Some(AttemptResult::Returned);
            value.summary = Some("dependency completed".into());
        }
        let movement = (task.state != TaskState::Todo).then(|| TaskMove {
            actor: "system".into(),
            from: task.state.clone(),
            to: TaskState::Todo,
            reason: "dependency completed".into(),
            at: at.clone(),
        });
        let comment = task.owner_pane_id.as_ref().map(|_| Comment {
            comment_id: Uuid::new_v4().to_string(),
            task_id: task.task_id.clone(),
            author: "system".into(),
            text: "dependency completed; task returned".into(),
            at,
        });
        let change = TaskBlockingChange {
            expected_version: task.version,
            task: updated,
            attempt,
            movement,
            comment,
            next_retry_ms: None,
        };
        self.commit_block_change(task.owner_pane_id.as_deref(), change, None, None)
    }

    fn advance_external_block(&mut self, task: Task) -> Result<()> {
        let owner = task.owner_pane_id.clone();
        let owner_alive = owner.as_deref().is_some_and(|pane| {
            self.panes.get(pane).is_some_and(|running| {
                !running.stored.closed && running.stored.pane.state != PaneState::Dead
            }) && self.agents.contains_key(pane)
        });
        match external_action(&task, owner_alive)? {
            BlockingAction::Retry(updated) => self.retry_external_block(task, updated),
            BlockingAction::NeedsInformation(updated) => {
                self.persist_block_change(task, updated, None, true)
            }
            _ => Ok(()),
        }
    }

    fn retry_external_block(&mut self, current: Task, updated: Task) -> Result<()> {
        let pane = errors::invariant(
            current.owner_pane_id.as_deref(),
            "Retryable task has no executor",
        )?;
        let mut state = self.agent_state(pane)?;
        if state.queue.len() >= limits::PROMPT_QUEUE_COUNT {
            return self.reschedule_block(&current);
        }
        let blocking = errors::invariant(
            current.blocking.as_ref(),
            "Retryable task lost its blocking value",
        )?;
        let prompt = state.enqueue(
            format!(
                "Retry the task after this external error: {}",
                blocking.reason
            ),
            PromptOrigin::Automatic,
            self.timestamp(),
        )?;
        let change = TaskBlockingChange {
            expected_version: current.version,
            task: updated,
            attempt: None,
            movement: None,
            comment: None,
            next_retry_ms: None,
        };
        self.commit_block_change(Some(pane), change, Some((state, prompt.prompt_id)), None)
    }

    fn reschedule_block(&mut self, task: &Task) -> Result<()> {
        let workspace = task.workspace_id.clone();
        let task_id = task.task_id.clone();
        let due = self.now_ms().saturating_add(RETRY_DELAY_MS);
        StorePort::mutate(
            &mut self.store,
            "engine-task",
            &Uuid::new_v4().to_string(),
            "task.block.retry_deferred",
            |change| {
                change.schedule_task_retry(&workspace, &task_id, due)?;
                Ok((json!({}), Vec::new()))
            },
        )
        .map_err(errors::storage)?;
        Ok(())
    }

    fn persist_block_change(
        &mut self,
        current: Task,
        updated: Task,
        next_retry_ms: Option<i64>,
        needs_attention: bool,
    ) -> Result<()> {
        let owner = current.owner_pane_id.clone();
        let change = TaskBlockingChange {
            expected_version: current.version,
            task: updated,
            attempt: None,
            movement: None,
            comment: None,
            next_retry_ms,
        };
        self.commit_block_change(
            owner.as_deref(),
            change,
            None,
            needs_attention.then_some(()),
        )
    }

    fn commit_block_change(
        &mut self,
        pane: Option<&str>,
        change: TaskBlockingChange,
        queued: Option<(State, String)>,
        needs_attention: Option<()>,
    ) -> Result<()> {
        if let Some(pane) = pane
            && self.agents.contains_key(pane)
            && self
                .panes
                .get(pane)
                .is_some_and(|value| !value.stored.closed)
        {
            return self.commit_block_for_agent(pane, change, queued, needs_attention);
        }
        self.commit_block_without_agent(change)
    }

    fn commit_block_for_agent(
        &mut self,
        pane: &str,
        change: TaskBlockingChange,
        queued: Option<(State, String)>,
        needs_attention: Option<()>,
    ) -> Result<()> {
        let task_for_attention = change.task.clone();
        let attention_id = format!("task:{}:blocking", change.task.task_id);
        let state = queued
            .as_ref()
            .map_or_else(|| self.agent_state(pane), |(state, _)| Ok(state.clone()))?;
        let saved = self.panes[pane].stored.clone();
        let mut emitted = change.events();
        if queued.is_some() {
            let prompt_ids = state.queue.iter().map(|item| item.prompt_id.clone());
            emitted.push(crate::model::pane_queued(
                pane,
                state.queue.len(),
                prompt_ids.collect(),
                false,
            )?);
        }
        if needs_attention.is_some() {
            emitted.push(events::attention(&task_for_attention));
        }
        let mut changes = Changes {
            task_blocking: Some(change),
            events: emitted,
            ..Changes::default()
        };
        if needs_attention.is_some() {
            changes
                .attention
                .push(attention(&task_for_attention, pane, self.timestamp()));
        } else {
            changes.remove_attention.push(attention_id);
        }
        self.commit_agent(saved, state, changes, None, json!({}))?;
        Ok(())
    }

    fn commit_block_without_agent(&mut self, change: TaskBlockingChange) -> Result<()> {
        let emitted = change.events();
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-task",
            &Uuid::new_v4().to_string(),
            "task.block.maintenance",
            |transaction| {
                change.persist(transaction)?;
                Ok((json!({}), emitted))
            },
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::task_fixture;

    #[test]
    fn dependency_completion_and_cancellation_have_distinct_outcomes() {
        let mut blocked = task_fixture(TaskState::InProgress);
        blocked.blocking = Some(Blocking {
            kind: BlockingKind::Dependency,
            reason: "waiting".into(),
            dependency_task_id: Some("WS-2".into()),
            retry_count: None,
        });
        let completed = task_fixture(TaskState::Completed);
        let BlockingAction::DependencyCompleted(released) =
            dependency_action(&blocked, &completed).unwrap()
        else {
            panic!("completed dependency should release the task")
        };
        assert_eq!(released.state, TaskState::Todo);
        assert!(released.owner_pane_id.is_none());
        assert!(released.blocking.is_none());

        let cancelled = task_fixture(TaskState::Cancelled);
        let BlockingAction::DependencyCancelled(waiting) =
            dependency_action(&blocked, &cancelled).unwrap()
        else {
            panic!("cancelled dependency should require a decision")
        };
        assert_eq!(waiting.state, TaskState::InProgress);
        assert_eq!(waiting.owner_pane_id.as_deref(), Some("pane-1"));
        assert_eq!(waiting.phrase, "blocked: dependency cancelled");
    }

    #[test]
    fn external_error_retries_twice_then_needs_information() {
        let mut task = task_fixture(TaskState::InProgress);
        task.blocking = Some(Blocking {
            kind: BlockingKind::ExternalError,
            reason: "service unavailable".into(),
            dependency_task_id: None,
            retry_count: Some(0),
        });
        for expected in [1, 2] {
            let BlockingAction::Retry(next) = external_action(&task, true).unwrap() else {
                panic!("external error should retry")
            };
            assert_eq!(
                next.blocking.as_ref().and_then(|value| value.retry_count),
                Some(expected)
            );
            task = next;
        }
        let BlockingAction::NeedsInformation(waiting) = external_action(&task, true).unwrap()
        else {
            panic!("third decision should require a person")
        };
        assert_eq!(
            waiting.blocking.unwrap().kind,
            BlockingKind::MissingInformation
        );
    }
}

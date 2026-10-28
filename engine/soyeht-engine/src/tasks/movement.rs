use super::{api::task, apply_move, events};
use crate::{
    agents::{Changes, methods::cancel},
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Attempt, AttemptResult, AttentionItem, ErrorKind, Role, Task, TaskMove, TaskMoveParams,
    TaskState, method,
};
use soyeht_store::{
    agents,
    database::{Change, Result as StoreResult},
};
use uuid::Uuid;

fn closing_result(state: &TaskState) -> AttemptResult {
    match state {
        TaskState::Completed => AttemptResult::Completed,
        TaskState::Cancelled => AttemptResult::Cancelled,
        _ => AttemptResult::Returned,
    }
}

pub(crate) struct TaskHumanMove {
    expected_version: u32,
    task: Task,
    movement: TaskMove,
    attempt: Option<Attempt>,
}

impl TaskHumanMove {
    pub(crate) fn persist(&self, change: &Change<'_>) -> StoreResult<()> {
        if let Some(attempt) = &self.attempt {
            change.replace_attempt(&self.task.workspace_id, &self.task.task_id, attempt)?;
        }
        change.replace_task(&self.task, self.expected_version)?;
        change.insert_move(&self.task.workspace_id, &self.task.task_id, &self.movement)
    }

    pub(crate) fn events(&self) -> Vec<(String, Value)> {
        vec![
            events::moved(&self.task, &self.movement),
            events::updated(&self.task),
            events::phrase_changed(&self.task),
        ]
    }
}

impl Core {
    pub(super) fn task_move(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskMoveParams,
    ) -> Result<Value> {
        let actor = self.task_actor(connection)?;
        if !matches!(actor.role, Role::HumanLocal | Role::HumanDevice) {
            return Err(errors::forbidden_role("Only a person may move task cards"));
        }
        let current = task(&self.store, &params.workspace_id, &params.task_id)?;
        let subitems = self
            .store
            .task_subitems(&params.workspace_id, &params.task_id)
            .map_err(errors::storage)?;
        let reason = params.reason.trim().to_owned();
        let move_result = apply_move(
            &current,
            params.state,
            self.actor_name(&actor)?,
            reason.clone(),
            self.timestamp(),
            &subitems,
        );
        let (updated, movement) = match move_result {
            Ok(value) => value,
            Err(error) => {
                if errors::kind(&error) == ErrorKind::TransitionRefused {
                    self.record_refused_move(&current, &error.to_string())?;
                }
                return Err(error);
            }
        };
        let refused_attention = self.refused_move_attention(&current)?;
        let attempt = self.closed_attempt_for_move(&current, &updated, &reason)?;
        let change = TaskHumanMove {
            expected_version: current.version,
            task: updated,
            movement,
            attempt,
        };
        if let Some(pane) = current.owner_pane_id.as_deref()
            && change.task.owner_pane_id.is_none()
        {
            return self.move_owned_task(connection, raw, pane, change, refused_attention);
        }
        self.persist_human_move(connection, raw, change, refused_attention)
    }

    fn refused_move_attention(&self, task: &Task) -> Result<Option<AttentionItem>> {
        let attention_id = format!("task:{}:move-refused", task.task_id);
        Ok(agents::attention(&self.store, &task.workspace_id)
            .map_err(errors::storage)?
            .into_iter()
            .find(|item| item.attention_id == attention_id))
    }

    fn record_refused_move(&mut self, task: &Task, reason: &str) -> Result<()> {
        let Some(pane) = task.owner_pane_id.as_deref() else {
            return Ok(());
        };
        let prior = self.refused_move_attention(task)?;
        let item = AttentionItem {
            attention_id: format!("task:{}:move-refused", task.task_id),
            pane_id: pane.into(),
            request_id: None,
            message: format!("Move refused for {}: {reason}", task.task_id),
            created_at: prior
                .as_ref()
                .map_or_else(|| self.timestamp(), |item| item.created_at.clone()),
        };
        if prior.as_ref() == Some(&item) {
            return Ok(());
        }
        let workspace = task.workspace_id.clone();
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-task-move",
            &Uuid::new_v4().to_string(),
            "refused-task-move",
            |change| {
                agents::save_attention(change, &workspace, &item)?;
                let items = agents::attention_in_change(change, &workspace)?;
                Ok((
                    json!({}),
                    vec![
                        events::attention(task),
                        events::attention_changed(&workspace, &items),
                    ],
                ))
            },
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)
    }

    fn closed_attempt_for_move(
        &self,
        current: &Task,
        updated: &Task,
        reason: &str,
    ) -> Result<Option<Attempt>> {
        if current.owner_pane_id.is_none() || updated.owner_pane_id.is_some() {
            return Ok(None);
        }
        let mut attempt = self.open_attempt(current)?;
        attempt.end = Some(self.timestamp());
        attempt.result = Some(closing_result(&updated.state));
        attempt.summary = Some(reason.into());
        Ok(Some(attempt))
    }

    fn move_owned_task(
        &mut self,
        connection: &str,
        raw: &Value,
        pane: &str,
        movement: TaskHumanMove,
        refused_attention: Option<AttentionItem>,
    ) -> Result<Value> {
        let mut state = self.agent_state(pane)?;
        let mut saved = self.panes[pane].stored.clone();
        let mut changes = Changes {
            task_human_move: Some(movement),
            ..Changes::default()
        };
        if let Some(item) = refused_attention {
            changes.remove_attention.push(item.attention_id);
        }
        if state.active.is_some() {
            cancel(
                &self.store,
                pane,
                &mut state,
                &mut saved,
                &mut changes,
                self.now_ms(),
                &self.timestamp(),
            )?;
        }
        let task = changes
            .task_human_move
            .as_ref()
            .map(|change| change.task.clone())
            .ok_or_else(|| errors::internal("Task move disappeared before commit"))?;
        Ok(self
            .commit_agent(
                saved,
                state,
                changes,
                Some((connection, method::TASK_MOVE, raw)),
                json!({"task":task}),
            )?
            .result)
    }

    fn persist_human_move(
        &mut self,
        connection: &str,
        raw: &Value,
        movement: TaskHumanMove,
        refused_attention: Option<AttentionItem>,
    ) -> Result<Value> {
        let result = json!({"task":movement.task});
        let mutation = self.mutate(connection, method::TASK_MOVE, raw, move |change| {
            movement.persist(change)?;
            let mut events = movement.events();
            if let Some(item) = &refused_attention {
                change.remove_attention(&item.pane_id, &item.attention_id)?;
                let items = agents::attention_in_change(change, &movement.task.workspace_id)?;
                events.push(events::attention_changed(
                    &movement.task.workspace_id,
                    &items,
                ));
            }
            Ok((result, events))
        })?;
        Ok(mutation.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_results_distinguish_return_cancel_and_completion() {
        assert_eq!(closing_result(&TaskState::Todo), AttemptResult::Returned);
        assert_eq!(
            closing_result(&TaskState::Cancelled),
            AttemptResult::Cancelled
        );
        assert_eq!(
            closing_result(&TaskState::Completed),
            AttemptResult::Completed
        );
    }
}

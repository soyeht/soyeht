use super::{api::task, apply_move};
use crate::{
    agents::{Changes, methods::cancel},
    core::Core,
    errors::{self, Result},
};
use serde_json::{Value, json};
use soyeht_protocol::{Attempt, AttemptResult, Role, Task, TaskMove, TaskMoveParams, TaskState};
use soyeht_store::database::{Change, Result as StoreResult};

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
            (
                "task.moved".into(),
                json!({"workspaceId":self.task.workspace_id,"taskId":self.task.task_id,
                    "actor":self.movement.actor,"from":self.movement.from,
                    "to":self.movement.to,"reason":self.movement.reason}),
            ),
            (
                "task.updated".into(),
                json!({"workspaceId":self.task.workspace_id,"taskId":self.task.task_id,
                    "version":self.task.version}),
            ),
            (
                "task.phrase_changed".into(),
                json!({"workspaceId":self.task.workspace_id,"taskId":self.task.task_id,
                    "phrase":self.task.phrase}),
            ),
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
        let (updated, movement) = apply_move(
            &current,
            params.state,
            self.actor_name(&actor)?,
            reason.clone(),
            self.timestamp(),
            &subitems,
        )?;
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
            return self.move_owned_task(connection, raw, pane, change);
        }
        self.persist_human_move(connection, raw, change)
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
    ) -> Result<Value> {
        let mut state = self.agent_state(pane)?;
        let mut saved = self.panes[pane].stored.clone();
        let mut changes = Changes {
            task_human_move: Some(movement),
            ..Changes::default()
        };
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
                Some((connection, "task.move", raw)),
                json!({"task":task}),
            )?
            .result)
    }

    fn persist_human_move(
        &mut self,
        connection: &str,
        raw: &Value,
        movement: TaskHumanMove,
    ) -> Result<Value> {
        let result = json!({"task":movement.task});
        let mutation = self.mutate(connection, "task.move", raw, move |change| {
            movement.persist(change)?;
            Ok((result, movement.events()))
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

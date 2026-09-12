use super::{
    api::{parameters, task},
    apply_block, apply_complete, apply_release, apply_request_review,
};
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Actor, Attempt, AttemptResult, BlockingKind, Role, Task, TaskBlockParams, TaskCompleteParams,
    TaskReleaseParams, TaskRequestReviewParams, TaskState, TaskUnblockParams,
};

struct ClosingMove {
    current: Task,
    updated: Task,
    attempt: Attempt,
    actor: String,
    reason: String,
    released: Option<Value>,
}

struct ActiveMove {
    current: Task,
    updated: Task,
    actor: String,
    reason: String,
}

impl Core {
    pub(super) fn task_lifecycle(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        match method {
            "task.block" => self.task_block(connection, params, parameters(params)?),
            "task.release" => self.task_release(connection, params, parameters(params)?),
            "task.request_review" => {
                self.task_request_review(connection, params, parameters(params)?)
            }
            "task.complete" => self.task_complete(connection, params, parameters(params)?),
            "task.unblock" => self.task_unblock(connection, params, parameters(params)?),
            _ => Err(errors::invalid("Unknown task lifecycle method")),
        }
    }

    fn executor_task(
        &self,
        connection: &str,
        raw: &Value,
        workspace: &str,
        task_id: &str,
    ) -> Result<(Actor, Task)> {
        let actor = self.task_actor(connection)?;
        if actor.role != Role::Agent {
            return Err(errors::forbidden_role(
                "Only the active executor may use this task operation",
            ));
        }
        let item = task(&self.store, workspace, task_id)?;
        self.authorize_task_read(&actor, &item)?;
        self.authorize_executor(&actor, &item, raw)?;
        Ok((actor, item))
    }

    fn open_attempt(&self, task: &Task) -> Result<Attempt> {
        self.store
            .task_attempts(&task.workspace_id, &task.task_id)
            .map_err(errors::storage)?
            .into_iter()
            .find(|attempt| attempt.n == task.current_attempt && attempt.end.is_none())
            .ok_or_else(|| errors::internal("Active task has no open attempt"))
    }

    fn task_block(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskBlockParams,
    ) -> Result<Value> {
        let (_, current) =
            self.executor_task(connection, raw, &params.workspace_id, &params.task_id)?;
        self.validate_block_dependency(&params)?;
        let updated = apply_block(
            &current,
            params.kind.clone(),
            params.reason.trim().into(),
            params.dependency_task_id,
        )?;
        let workspace = params.workspace_id;
        let task_id = params.task_id;
        let kind = params.kind;
        let reason = params.reason.trim().to_owned();
        let expected = current.version;
        let mutation = self.mutate(connection, "task.block", raw, move |change| {
            change.replace_task(&updated, expected)?;
            Ok((
                json!({"task":updated}),
                vec![
                    (
                        "task.blocked".into(),
                        json!({"workspaceId":workspace,"taskId":task_id,"kind":kind,"reason":reason}),
                    ),
                    (
                        "task.phrase_changed".into(),
                        json!({"workspaceId":workspace,"taskId":task_id,"phrase":updated.phrase}),
                    ),
                ],
            ))
        })?;
        Ok(mutation.result)
    }

    fn validate_block_dependency(&self, params: &TaskBlockParams) -> Result<()> {
        match (&params.kind, &params.dependency_task_id) {
            (BlockingKind::Dependency, Some(dependency)) => {
                if dependency == &params.task_id {
                    return Err(errors::dependency_open("A task cannot block on itself"));
                }
                let dependency = self
                    .store
                    .task(&params.workspace_id, dependency)
                    .map_err(errors::storage)?
                    .ok_or_else(|| errors::dependency_open("Blocking dependency is unavailable"))?;
                if matches!(
                    dependency.state,
                    TaskState::Completed | TaskState::Cancelled
                ) {
                    return Err(errors::dependency_open(
                        "Blocking dependency is already closed",
                    ));
                }
                Ok(())
            }
            (BlockingKind::Dependency, None) => Err(errors::dependency_open(
                "Dependency blocking needs a task id",
            )),
            (_, Some(_)) => Err(errors::invalid(
                "dependencyTaskId is valid only for dependency blocking",
            )),
            (_, None) => Ok(()),
        }
    }

    fn task_request_review(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskRequestReviewParams,
    ) -> Result<Value> {
        let (actor, current) =
            self.executor_task(connection, raw, &params.workspace_id, &params.task_id)?;
        let subitems = self
            .store
            .task_subitems(&params.workspace_id, &params.task_id)
            .map_err(errors::storage)?;
        let updated = apply_request_review(&current, &subitems)?;
        let actor = self.actor_name(&actor)?;
        let reason = params.reason.unwrap_or_else(|| "review requested".into());
        self.persist_active_move(
            connection,
            "task.request_review",
            raw,
            ActiveMove {
                current,
                updated,
                actor,
                reason,
            },
        )
    }

    fn task_complete(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskCompleteParams,
    ) -> Result<Value> {
        let (actor, current) =
            self.executor_task(connection, raw, &params.workspace_id, &params.task_id)?;
        let mut attempt = self.open_attempt(&current)?;
        let updated = apply_complete(&current, &params)?;
        attempt.end = Some(self.timestamp());
        attempt.result = Some(AttemptResult::Completed);
        attempt.summary = Some(params.summary.clone());
        let actor = self.actor_name(&actor)?;
        self.persist_closing_move(
            connection,
            "task.complete",
            raw,
            ClosingMove {
                current,
                updated,
                attempt,
                actor,
                reason: "completed".into(),
                released: None,
            },
        )
    }

    fn task_release(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskReleaseParams,
    ) -> Result<Value> {
        let (actor, current) =
            self.executor_task(connection, raw, &params.workspace_id, &params.task_id)?;
        let mut attempt = self.open_attempt(&current)?;
        let updated = apply_release(&current)?;
        let reason = params
            .reason
            .unwrap_or_else(|| "returned by executor".into());
        attempt.end = Some(self.timestamp());
        attempt.result = Some(AttemptResult::Returned);
        attempt.summary = Some(reason.clone());
        let event = Some(json!({
            "workspaceId":params.workspace_id,"taskId":params.task_id,
            "reason":reason,"attempt":current.current_attempt
        }));
        let actor = self.actor_name(&actor)?;
        self.persist_closing_move(
            connection,
            "task.release",
            raw,
            ClosingMove {
                current,
                updated,
                attempt,
                actor,
                reason,
                released: event,
            },
        )
    }

    fn task_unblock(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskUnblockParams,
    ) -> Result<Value> {
        let actor = self.task_actor(connection)?;
        if !matches!(
            actor.role,
            Role::HumanLocal | Role::HumanDevice | Role::Chief
        ) {
            return Err(errors::forbidden_role(
                "Only a person or lead agent may unblock a task",
            ));
        }
        let current = task(&self.store, &params.workspace_id, &params.task_id)?;
        let blocking = current
            .blocking
            .as_ref()
            .ok_or_else(|| errors::transition_refused("Task is not blocked"))?;
        if blocking.kind == BlockingKind::Dependency
            && let Some(dependency) = blocking.dependency_task_id.as_deref()
            && !matches!(
                task(&self.store, &params.workspace_id, dependency)?.state,
                TaskState::Completed
            )
        {
            return Err(errors::dependency_open("Task dependency is still open"));
        }
        let updated = super::apply_unblock(&current)?;
        let workspace = params.workspace_id;
        let task_id = params.task_id;
        let reason = params.reason.unwrap_or_else(|| "unblocked".into());
        let expected = current.version;
        let mutation = self.mutate(connection, "task.unblock", raw, move |change| {
            change.replace_task(&updated, expected)?;
            Ok((
                json!({"task":updated}),
                vec![
                    (
                        "task.unblocked".into(),
                        json!({"workspaceId":workspace,"taskId":task_id,"reason":reason}),
                    ),
                    (
                        "task.phrase_changed".into(),
                        json!({"workspaceId":workspace,"taskId":task_id,"phrase":updated.phrase}),
                    ),
                ],
            ))
        })?;
        Ok(mutation.result)
    }

    fn persist_active_move(
        &mut self,
        connection: &str,
        method: &str,
        raw: &Value,
        active: ActiveMove,
    ) -> Result<Value> {
        let movement = soyeht_protocol::TaskMove {
            actor: active.actor,
            from: active.current.state,
            to: active.updated.state.clone(),
            reason: active.reason,
            at: self.timestamp(),
        };
        let workspace = active.updated.workspace_id.clone();
        let task_id = active.updated.task_id.clone();
        let expected = active.current.version;
        let mutation = self.mutate(connection, method, raw, move |change| {
            change.replace_task(&active.updated, expected)?;
            change.insert_move(&workspace, &task_id, &movement)?;
            Ok((
                json!({"task":active.updated}),
                vec![
                    (
                        "task.moved".into(),
                        json!({"workspaceId":workspace,"taskId":task_id,
                            "actor":movement.actor,"from":movement.from,
                            "to":movement.to,"reason":movement.reason}),
                    ),
                    (
                        "task.phrase_changed".into(),
                        json!({"workspaceId":workspace,"taskId":task_id,"phrase":active.updated.phrase}),
                    ),
                ],
            ))
        })?;
        Ok(mutation.result)
    }

    fn persist_closing_move(
        &mut self,
        connection: &str,
        method: &str,
        raw: &Value,
        closing: ClosingMove,
    ) -> Result<Value> {
        let at = errors::invariant(
            closing.attempt.end.clone(),
            "Closing task attempt has no end time",
        )?;
        let movement = soyeht_protocol::TaskMove {
            actor: closing.actor,
            from: closing.current.state,
            to: closing.updated.state.clone(),
            reason: closing.reason,
            at,
        };
        let workspace = closing.updated.workspace_id.clone();
        let task_id = closing.updated.task_id.clone();
        let expected = closing.current.version;
        let mutation = self.mutate(connection, method, raw, move |change| {
            change.replace_task(&closing.updated, expected)?;
            change.replace_attempt(&workspace, &task_id, &closing.attempt)?;
            change.insert_move(&workspace, &task_id, &movement)?;
            let mut events = vec![
                (
                    "task.moved".into(),
                    json!({"workspaceId":workspace,"taskId":task_id,
                        "actor":movement.actor,"from":movement.from,
                        "to":movement.to,"reason":movement.reason}),
                ),
                (
                    "task.phrase_changed".into(),
                    json!({"workspaceId":workspace,"taskId":task_id,"phrase":closing.updated.phrase}),
                ),
            ];
            if let Some(released) = closing.released {
                events.push(("task.released".into(), released));
            }
            Ok((json!({"task":closing.updated}), events))
        })?;
        Ok(mutation.result)
    }
}

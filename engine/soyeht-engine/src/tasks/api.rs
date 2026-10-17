use super::{
    ExecutorIdentity, apply_update, new_task, ports::TaskStorePort, validate_dependencies,
    validate_executor,
};
use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Result},
    model,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use soyeht_protocol::{
    Actor, Comment, Role, Subitem, SubitemState, Task, TaskCommentParams, TaskCommentsParams,
    TaskCommentsResult, TaskCreateParams, TaskCreateResult, TaskDispatchParams, TaskDispatchResult,
    TaskExecutor, TaskListParams, TaskListResult, TaskShowParams, TaskSubitemAddParams,
    TaskSubitemDoneParams, TaskUpdateParams, event, method,
};
use soyeht_store::database::Error as StoreError;
use uuid::Uuid;

pub(super) fn parameters<T: DeserializeOwned>(params: &Value) -> Result<T> {
    serde_json::from_value(params.clone())
        .map_err(|_| errors::invalid("Task parameters do not match the protocol"))
}

pub(super) fn task(store: &impl TaskStorePort, workspace: &str, task_id: &str) -> Result<Task> {
    store
        .task(workspace, task_id)
        .map_err(errors::storage)?
        .ok_or_else(|| errors::not_found("Task not found"))
}

fn cursor(value: Option<&str>, name: &str) -> Result<u32> {
    value
        .map(|raw| {
            raw.parse::<u32>()
                .map_err(|_| errors::invalid(format!("Invalid {name} cursor")))
        })
        .transpose()
        .map(|value| value.unwrap_or(0))
}

impl Core {
    pub fn task_query(&self, connection: &str, method: &str, params: &Value) -> Result<Value> {
        match method {
            method::TASK_SHOW => self.task_show(connection, parameters(params)?),
            method::TASK_LIST => self.task_list(connection, parameters(params)?),
            method::TASK_COMMENTS => self.task_comments(connection, parameters(params)?),
            _ => Err(errors::invalid("Unknown task query")),
        }
    }

    pub fn task_author(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Outcome> {
        if method == method::TASK_CREATE {
            return self.task_create(connection, params, parameters(params)?);
        }
        self.task_edit(connection, method, params)
            .map(Outcome::Reply)
    }

    fn task_edit(&mut self, connection: &str, method: &str, params: &Value) -> Result<Value> {
        match method {
            method::TASK_UPDATE => self.task_update(connection, params, parameters(params)?),
            method::TASK_COMMENT => self.task_comment(connection, params, parameters(params)?),
            method::TASK_SUBITEM_ADD => {
                self.task_subitem_add(connection, params, parameters(params)?)
            }
            method::TASK_SUBITEM_DONE => {
                self.task_subitem_done(connection, params, parameters(params)?)
            }
            method::TASK_BLOCK
            | method::TASK_RELEASE
            | method::TASK_COMPLETE
            | method::TASK_UNBLOCK => self.task_lifecycle(connection, method, params),
            method::TASK_MOVE => self.task_move(connection, params, parameters(params)?),
            _ => Err(errors::invalid("Unknown task authoring method")),
        }
    }

    pub(super) fn task_actor(&self, connection: &str) -> Result<Actor> {
        self.clients
            .get(connection)
            .and_then(|client| client.actor.clone())
            .ok_or_else(|| errors::unauthorized("Task connection is not authenticated"))
    }

    pub(super) fn actor_name(&self, actor: &Actor) -> Result<String> {
        match actor.role {
            Role::HumanLocal | Role::HumanDevice => Ok("you".into()),
            Role::Agent | Role::Chief => {
                let pane = actor
                    .pane_id
                    .as_deref()
                    .and_then(|pane| self.panes.get(pane))
                    .ok_or_else(|| errors::unauthorized("Bound agent pane is unavailable"))?;
                Ok(pane.stored.pane.name.clone())
            }
        }
    }

    pub(super) fn authorize_task_read(&self, actor: &Actor, item: &Task) -> Result<()> {
        if actor.role == Role::Agent && actor.pane_id.as_deref() != item.owner_pane_id.as_deref() {
            return Err(errors::forbidden_role(
                "An agent may read only the task bound to its pane",
            ));
        }
        Ok(())
    }

    pub(super) fn authorize_executor(
        &self,
        actor: &Actor,
        item: &Task,
        params: &Value,
    ) -> Result<()> {
        if actor.role != Role::Agent {
            return Ok(());
        }
        let pane_id = errors::param_str(params, "paneId")?;
        if actor.pane_id.as_deref() != Some(pane_id) {
            return Err(errors::stale_attempt(
                "Executor pane does not match the authenticated binding",
            ));
        }
        let generation = u32::try_from(errors::param_u64(params, "sessionGeneration")?)
            .map_err(|_| errors::stale_attempt("Session generation exceeds its range"))?;
        let attempt = u32::try_from(errors::param_u64(params, "attempt")?)
            .map_err(|_| errors::stale_attempt("Task attempt exceeds its range"))?;
        let active_generation = self
            .panes
            .get(pane_id)
            .filter(|pane| !pane.stored.closed)
            .map(|pane| pane.stored.pane.generation)
            .ok_or_else(|| errors::stale_attempt("Executor pane is not active"))?;
        validate_executor(
            item,
            &ExecutorIdentity {
                pane_id,
                session_generation: generation,
                attempt,
            },
            active_generation,
        )
    }

    fn task_show(&self, connection: &str, params: TaskShowParams) -> Result<Value> {
        let actor = self.task_actor(connection)?;
        let item = task(&self.store, &params.workspace_id, &params.task_id)?;
        self.authorize_task_read(&actor, &item)?;
        let subitems = self
            .store
            .subitems(&params.workspace_id, &params.task_id)
            .map_err(errors::storage)?;
        let comments = self
            .store
            .comments(&params.workspace_id, &params.task_id, None, 200)
            .map_err(errors::storage)?;
        let attempts = self
            .store
            .attempts(&params.workspace_id, &params.task_id)
            .map_err(errors::storage)?;
        let history = self
            .store
            .moves(&params.workspace_id, &params.task_id)
            .map_err(errors::storage)?;
        Ok(
            json!({"task":item,"subitems":subitems,"comments":comments,"attempts":attempts,"history":history}),
        )
    }

    fn task_list(&self, connection: &str, params: TaskListParams) -> Result<Value> {
        let actor = self.task_actor(connection)?;
        if actor.role == Role::Agent {
            return Err(errors::forbidden_role(
                "Executor agents cannot list workspace tasks",
            ));
        }
        let offset = cursor(params.cursor.as_deref(), "task")?;
        let fetch = params
            .limit
            .checked_add(1)
            .ok_or_else(|| errors::invalid("Task page limit exceeds its range"))?;
        let mut items = self
            .store
            .list_tasks(&params.workspace_id, params.state.as_ref(), fetch, offset)
            .map_err(errors::storage)?;
        let has_more = items.len() > params.limit as usize;
        items.truncate(params.limit as usize);
        let next = has_more
            .then(|| {
                offset
                    .checked_add(params.limit)
                    .ok_or_else(|| errors::invalid("Task cursor exceeds its range"))
                    .map(|value| value.to_string())
            })
            .transpose()?;
        serde_json::to_value(TaskListResult {
            items,
            next_cursor: next,
        })
        .map_err(|_| errors::internal("Task list result cannot be encoded"))
    }

    fn task_comments(&self, connection: &str, params: TaskCommentsParams) -> Result<Value> {
        let actor = self.task_actor(connection)?;
        let item = task(&self.store, &params.workspace_id, &params.task_id)?;
        self.authorize_task_read(&actor, &item)?;
        let before = params
            .before
            .as_deref()
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| errors::invalid("Invalid comment cursor"))
            })
            .transpose()?;
        let page = self
            .store
            .comments_page(&params.workspace_id, &params.task_id, before, params.limit)
            .map_err(errors::storage)?;
        serde_json::to_value(TaskCommentsResult {
            items: page.items,
            next_cursor: page.next_before.map(|value| value.to_string()),
        })
        .map_err(|_| errors::internal("Task comments result cannot be encoded"))
    }

    fn task_create(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskCreateParams,
    ) -> Result<Outcome> {
        let actor = self.task_actor(connection)?;
        if !matches!(
            actor.role,
            Role::HumanLocal | Role::HumanDevice | Role::Chief
        ) {
            return Err(errors::forbidden_role(
                "Executor agents cannot create tasks",
            ));
        }
        self.require_person_for_full_access(
            &actor.role,
            [
                params.permission_mode.as_deref(),
                params
                    .executor
                    .as_ref()
                    .and_then(|executor| executor.permission_mode.as_deref()),
            ],
        )?;
        let should_dispatch = params.dispatch == Some(true);
        let create_key = params.idempotency_key.clone();
        model::workspace(&self.store, &params.workspace_id)?;
        let actor_name = self.actor_name(&actor)?;
        let template = new_task(&params, "pending".into(), actor_name, self.timestamp())?;
        validate_dependencies(
            &self.store,
            &params.workspace_id,
            &template.task_id,
            &template.dependencies,
        )?;
        let workspace = params.workspace_id.clone();
        let mutation = self.mutate(connection, method::TASK_CREATE, raw, move |change| {
            let mut created = template;
            created.task_id = change.allocate_task_id(&workspace)?;
            if created.dependencies.contains(&created.task_id) {
                return Err(StoreError::TaskDependencyCycle);
            }
            change.insert_task(&created)?;
            Ok((
                json!({"task":created}),
                vec![(event::TASK_CREATED.into(), json!({"task":created}))],
            ))
        })?;
        if !should_dispatch {
            return Ok(Outcome::Reply(mutation.result));
        }
        self.dispatch_created_task(connection, &create_key, mutation.result)
    }

    fn dispatch_created_task(
        &mut self,
        connection: &str,
        create_key: &str,
        result: Value,
    ) -> Result<Outcome> {
        let created: TaskCreateResult = serde_json::from_value(result)
            .map_err(|_| errors::internal("Stored task creation result is invalid"))?;
        let preset = created.task.executor.clone().unwrap_or(TaskExecutor {
            workspace_default: true,
            harness: None,
            model: None,
            effort: None,
            permission_mode: None,
        });
        let key = URL_SAFE_NO_PAD.encode(crate::auth::token_hash(create_key));
        let request = TaskDispatchParams {
            workspace_id: created.task.workspace_id,
            task_id: created.task.task_id,
            preset,
            idempotency_key: format!("create-dispatch:{key}"),
        };
        let raw = serde_json::to_value(request)
            .map_err(|_| errors::internal("Task dispatch parameters could not be encoded"))?;
        self.task_dispatch(connection, &raw)?
            .map_reply(|dispatched| {
                let dispatched: TaskDispatchResult = serde_json::from_value(dispatched)
                    .map_err(|_| errors::internal("Stored task dispatch result is invalid"))?;
                serde_json::to_value(TaskCreateResult {
                    task: dispatched.task,
                    pane_id: Some(dispatched.pane_id),
                })
                .map_err(|_| errors::internal("Task creation result could not be encoded"))
            })
    }

    fn task_update(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskUpdateParams,
    ) -> Result<Value> {
        let actor = self.task_actor(connection)?;
        let current = task(&self.store, &params.workspace_id, &params.task_id)?;
        self.authorize_task_read(&actor, &current)?;
        self.authorize_executor(&actor, &current, raw)?;
        if let Some(dependencies) = &params.dependencies {
            validate_dependencies(
                &self.store,
                &params.workspace_id,
                &params.task_id,
                dependencies,
            )?;
        }
        let updated = apply_update(&current, &params)?;
        let expected = current.version;
        let phrase_changed = current.phrase != updated.phrase;
        let workspace = params.workspace_id;
        let task_id = params.task_id;
        let mutation = self.mutate(connection, method::TASK_UPDATE, raw, move |change| {
            change.replace_task(&updated, expected)?;
            let mut events = vec![(
                event::TASK_UPDATED.into(),
                json!({"workspaceId":workspace,"taskId":task_id,"version":updated.version}),
            )];
            if phrase_changed {
                events.push((
                    event::TASK_PHRASE_CHANGED.into(),
                    json!({"workspaceId":workspace,"taskId":task_id,"phrase":updated.phrase}),
                ));
            }
            Ok((json!({"task":updated}), events))
        })?;
        Ok(mutation.result)
    }

    fn task_comment(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskCommentParams,
    ) -> Result<Value> {
        if params.text.trim().is_empty() {
            return Err(errors::invalid("Comment text must not be empty"));
        }
        let actor = self.task_actor(connection)?;
        let item = task(&self.store, &params.workspace_id, &params.task_id)?;
        self.authorize_task_read(&actor, &item)?;
        self.authorize_executor(&actor, &item, raw)?;
        let comment = Comment {
            comment_id: Uuid::new_v4().to_string(),
            task_id: params.task_id.clone(),
            author: self.actor_name(&actor)?,
            text: params.text.trim().into(),
            at: self.timestamp(),
        };
        let workspace = params.workspace_id;
        let task_id = params.task_id;
        let mutation = self.mutate(connection, method::TASK_COMMENT, raw, move |change| {
            change.insert_comment(&workspace, &task_id, &comment)?;
            Ok((
                json!({"comment":comment}),
                vec![(
                    event::TASK_COMMENTED.into(),
                    json!({"workspaceId":workspace,"taskId":task_id,"comment":comment}),
                )],
            ))
        })?;
        Ok(mutation.result)
    }

    fn task_subitem_add(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskSubitemAddParams,
    ) -> Result<Value> {
        if params.text.trim().is_empty() {
            return Err(errors::invalid("Subitem text must not be empty"));
        }
        let actor = self.task_actor(connection)?;
        if actor.role == Role::Chief {
            return Err(errors::forbidden_role(
                "Lead agents cannot change task subitems",
            ));
        }
        let item = task(&self.store, &params.workspace_id, &params.task_id)?;
        self.authorize_task_read(&actor, &item)?;
        self.authorize_executor(&actor, &item, raw)?;
        let workspace = params.workspace_id;
        let task_id = params.task_id;
        let text = params.text.trim().to_owned();
        let agent = params.agent;
        let instructions = params.instructions;
        let mutation = self.mutate(connection, method::TASK_SUBITEM_ADD, raw, move |change| {
            let subitem = Subitem {
                subitem_id: change.allocate_subitem_id(&workspace, &task_id)?,
                task_id: task_id.clone(),
                text,
                agent,
                instructions,
                state: SubitemState::Open,
            };
            change.insert_subitem(&workspace, &task_id, &subitem)?;
            Ok((
                json!({"subitem":subitem}),
                vec![(
                    event::TASK_SUBITEM_CHANGED.into(),
                    json!({"workspaceId":workspace,"taskId":task_id,"subitem":subitem}),
                )],
            ))
        })?;
        Ok(mutation.result)
    }

    fn task_subitem_done(
        &mut self,
        connection: &str,
        raw: &Value,
        params: TaskSubitemDoneParams,
    ) -> Result<Value> {
        let actor = self.task_actor(connection)?;
        if actor.role == Role::Chief {
            return Err(errors::forbidden_role(
                "Lead agents cannot change task subitems",
            ));
        }
        let item = task(&self.store, &params.workspace_id, &params.task_id)?;
        self.authorize_task_read(&actor, &item)?;
        self.authorize_executor(&actor, &item, raw)?;
        let mut subitem = self
            .store
            .subitems(&params.workspace_id, &params.task_id)
            .map_err(errors::storage)?
            .into_iter()
            .find(|item| item.subitem_id == params.subitem_id)
            .ok_or_else(|| errors::not_found("Task subitem not found"))?;
        subitem.state = SubitemState::Done;
        let workspace = params.workspace_id;
        let task_id = params.task_id;
        let mutation = self.mutate(connection, method::TASK_SUBITEM_DONE, raw, move |change| {
            change.replace_subitem(&workspace, &task_id, &subitem)?;
            Ok((
                json!({"subitem":subitem}),
                vec![(
                    event::TASK_SUBITEM_CHANGED.into(),
                    json!({"workspaceId":workspace,"taskId":task_id,"subitem":subitem}),
                )],
            ))
        })?;
        Ok(mutation.result)
    }
}

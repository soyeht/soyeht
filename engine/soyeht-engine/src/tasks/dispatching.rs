//! Durable routing from task reservation to an agent pane.

use super::{api::parameters, looping, worktree};
use crate::{
    agents::{Changes, Running, state::State},
    core::Core,
    errors::{self, Result},
    model::{self, RunningPane, StoredPane},
    panes::persist_new_pane,
    ports::StorePort,
    screen::Screen,
};
use serde_json::{Value, json};
use soyeht_protocol::{
    Actor, Pane, PaneMode, PaneState, PermissionMode, PromptOrigin, Role, StateSource, Task,
    TaskDispatchParams, TaskExecutor, TaskMove, TaskNextParams, TaskState, TaskWorktree,
};
use soyeht_store::{
    database::Change,
    tasks::{DispatchCheckpoint, TaskReservation},
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(crate) struct TaskDispatchChange {
    current: Task,
    updated: Task,
    checkpoint: DispatchCheckpoint,
}

impl TaskDispatchChange {
    pub(crate) fn persist(&self, change: &Change<'_>) -> soyeht_store::Result<()> {
        change.replace_task(&self.updated, self.current.version)?;
        change.save_dispatch_checkpoint(
            &self.updated.workspace_id,
            &self.updated.task_id,
            &self.checkpoint,
        )
    }
}

impl Core {
    pub(crate) fn task_dispatch(&mut self, connection: &str, raw: &Value) -> Result<Value> {
        if let Some(result) = self.repeated_mutation(connection, "task.dispatch", raw)? {
            self.advance_task_dispatches()?;
            return Ok(result);
        }
        let params: TaskDispatchParams = parameters(raw)?;
        let actor = self.task_actor(connection)?;
        require_dispatch_role(&actor)?;
        let current = super::api::task(&self.store, &params.workspace_id, &params.task_id)?;
        ensure_dependencies_closed(self, &current)?;
        let preset = self.resolve_new_preset(&params.preset)?;
        let pane_id = Uuid::new_v4().to_string();
        let actor_name = self.actor_name(&actor)?;
        let result =
            self.reserve_selected(connection, raw, current, preset, pane_id, actor_name)?;
        self.advance_task_dispatches()?;
        Ok(result)
    }

    pub(crate) fn task_next(&mut self, connection: &str, raw: &Value) -> Result<Value> {
        if let Some(result) = self.repeated_mutation(connection, "task.next", raw)? {
            return Ok(result);
        }
        let params: TaskNextParams = parameters(raw)?;
        let actor = self.task_actor(connection)?;
        let pane_id = validate_next_caller(self, &actor, &params)?;
        let state = self.agent_state(&pane_id)?;
        let harness = state.launch.id.clone();
        let model = selected_option(&state, "model").unwrap_or_default();
        let workspace = params.workspace_id.clone();
        let timestamp = self.timestamp();
        let loop_context = self.task_next_loop(&pane_id)?;
        let branch_pane = pane_id.clone();
        let actor_name = self.actor_name(&actor)?;
        let mutation = self.mutate(connection, "task.next", raw, move |change| {
            let Some(task) =
                change.reserve_next_task(&workspace, &branch_pane, &harness, &model, &timestamp)?
            else {
                let events = looping::record_task_next(change, loop_context.as_ref(), true)?;
                return Ok((json!({"empty":true,"willRestartSession":false}), events));
            };
            let events = looping::record_task_next(change, loop_context.as_ref(), false)?;
            let movement = claimed_move(actor_name, TaskState::Todo, &timestamp);
            change.insert_move(&workspace, &task.task_id, &movement)?;
            change.save_dispatch_checkpoint(
                &workspace,
                &task.task_id,
                &reserved_checkpoint(&branch_pane, &task.task_id),
            )?;
            Ok((
                json!({"task":task,"empty":false,"willRestartSession":true}),
                events,
            ))
        })?;
        Ok(mutation.result)
    }

    fn reserve_selected(
        &mut self,
        connection: &str,
        raw: &Value,
        current: Task,
        preset: TaskExecutor,
        pane_id: String,
        actor: String,
    ) -> Result<Value> {
        if current.owner_pane_id.is_some() {
            return Err(errors::task_has_owner("Task is unavailable for dispatch"));
        }
        if current.blocking.is_some() {
            return Err(errors::transition_refused(
                "Blocked task cannot be dispatched",
            ));
        }
        if !matches!(current.state, TaskState::Todo | TaskState::InReview) {
            return Err(errors::transition_refused(
                "Task state cannot be dispatched",
            ));
        }
        let workspace = current.workspace_id.clone();
        let task_id = current.task_id.clone();
        let prior_state = current.state.clone();
        let timestamp = self.timestamp();
        let harness = required_harness(&preset)?.to_owned();
        let model = preset.model.clone().unwrap_or_default();
        let repository = PathBuf::from(model::workspace(&self.store, &workspace)?.path);
        let prepared = prepare_new_pane(
            self,
            &current,
            &pane_id,
            repository
                .to_str()
                .ok_or_else(|| errors::invalid("Workspace path must be UTF-8"))?,
            &preset,
        )?;
        let saved = prepared.0.clone();
        let state = prepared.1.clone();
        let reserved_pane = pane_id.clone();
        let mutation = self.mutate(connection, "task.dispatch", raw, move |change| {
            persist_new_pane(change, &saved, Some(&state))?;
            let task = change.reserve_task(
                &workspace,
                &task_id,
                &preset,
                &TaskReservation {
                    pane_id: &reserved_pane,
                    harness: &harness,
                    model: &model,
                    started_at: &timestamp,
                },
            )?;
            let movement = claimed_move(actor, prior_state, &timestamp);
            change.insert_move(&workspace, &task_id, &movement)?;
            change.save_dispatch_checkpoint(
                &workspace,
                &task_id,
                &reserved_checkpoint(&reserved_pane, &task_id),
            )?;
            Ok((
                json!({"task":task,"paneId":reserved_pane,"attempt":task.current_attempt,"resumed":false}),
                Vec::new(),
            ))
        })?;
        if !mutation.repeated {
            self.panes.insert(
                prepared.0.pane.pane_id.clone(),
                RunningPane {
                    stored: prepared.0,
                    screen: prepared.2,
                },
            );
            self.agents
                .insert(pane_id.clone(), Running::new(prepared.1));
        }
        Ok(mutation.result)
    }

    pub(crate) fn advance_task_dispatches(&mut self) -> Result<()> {
        let pending = self.store.dispatch_checkpoints().map_err(errors::storage)?;
        for (workspace, task, checkpoint) in pending {
            if checkpoint.stage != "prompt_accepted" {
                self.advance_task_dispatch(&workspace, &task, checkpoint)?;
            }
        }
        Ok(())
    }

    fn advance_task_dispatch(
        &mut self,
        workspace: &str,
        task_id: &str,
        mut checkpoint: DispatchCheckpoint,
    ) -> Result<()> {
        let task = super::api::task(&self.store, workspace, task_id)?;
        let pane_id = checkpoint
            .pane_id
            .clone()
            .ok_or_else(|| errors::internal("Dispatch checkpoint has no pane"))?;
        let repository = PathBuf::from(model::workspace(&self.store, workspace)?.path);
        if checkpoint.stage == "reserved" {
            let branch = checkpoint
                .branch
                .as_deref()
                .ok_or_else(|| errors::internal("Dispatch checkpoint has no branch"))?;
            let prepared = self.worktrees.prepare(&repository, branch)?;
            checkpoint.stage = "worktree_created".into();
            checkpoint.worktree_path = Some(prepared.path);
            checkpoint.branch = Some(prepared.branch);
            self.store
                .record_dispatch_checkpoint(workspace, task_id, &checkpoint)
                .map_err(errors::storage)?;
        }
        if checkpoint.stage == "worktree_created" {
            if !self.ensure_task_pane(&task, &pane_id, &checkpoint)? {
                return Ok(());
            }
            checkpoint.stage = "pane_opened".into();
            self.store
                .record_dispatch_checkpoint(workspace, task_id, &checkpoint)
                .map_err(errors::storage)?;
        }
        if checkpoint.stage == "pane_opened" {
            self.accept_task_prompt(task, checkpoint)?;
        }
        Ok(())
    }
}

fn require_dispatch_role(actor: &Actor) -> Result<()> {
    if matches!(
        actor.role,
        Role::HumanLocal | Role::HumanDevice | Role::Chief
    ) {
        Ok(())
    } else {
        Err(errors::forbidden_role(
            "Executor agents cannot dispatch tasks",
        ))
    }
}

fn ensure_dependencies_closed(core: &Core, task: &Task) -> Result<()> {
    for dependency in &task.dependencies {
        let dependency = super::api::task(&core.store, &task.workspace_id, dependency)?;
        if dependency.state != TaskState::Completed {
            return Err(errors::dependency_open("Task has an open dependency"));
        }
    }
    Ok(())
}

fn validate_next_caller(core: &Core, actor: &Actor, params: &TaskNextParams) -> Result<String> {
    if actor.role != Role::Agent || actor.pane_id.as_deref() != Some(&params.pane_id) {
        return Err(errors::forbidden_role(
            "task.next requires the bound executor pane",
        ));
    }
    let pane = core
        .panes
        .get(&params.pane_id)
        .filter(|pane| !pane.stored.closed)
        .ok_or_else(|| errors::stale_attempt("Executor pane is unavailable"))?;
    let state = core.agent_state(&params.pane_id)?;
    if pane.stored.pane.workspace_id != params.workspace_id
        || pane.stored.pane.generation != params.session_generation
        || state.active.is_none()
        || state.operation.is_some()
    {
        return Err(errors::stale_attempt(
            "task.next does not belong to the active turn",
        ));
    }
    if core
        .store
        .active_task_for_pane(&params.pane_id)
        .map_err(errors::storage)?
        .is_some()
    {
        return Err(errors::task_has_owner("Pane already owns an active task"));
    }
    Ok(params.pane_id.clone())
}

fn claimed_move(actor: String, from: TaskState, at: &str) -> TaskMove {
    TaskMove {
        actor,
        from,
        to: TaskState::InProgress,
        reason: "Task claimed".into(),
        at: at.into(),
    }
}

fn reserved_checkpoint(pane: &str, task: &str) -> DispatchCheckpoint {
    DispatchCheckpoint {
        stage: "reserved".into(),
        worktree_path: None,
        branch: Some(worktree::task_branch(task)),
        pane_id: Some(pane.into()),
    }
}

fn required_harness(preset: &TaskExecutor) -> Result<&str> {
    preset
        .harness
        .as_deref()
        .ok_or_else(|| errors::harness_unavailable("Task preset has no configured harness"))
}

fn selected_option(state: &State, name: &str) -> Option<String> {
    state
        .options
        .values
        .iter()
        .find(|option| option.id == name)
        .and_then(|option| match &option.current_value {
            soyeht_protocol::ConfigValue::V0(value) => Some(value.clone()),
            _ => None,
        })
}

impl Core {
    fn resolve_new_preset(&self, requested: &TaskExecutor) -> Result<TaskExecutor> {
        let mut preset = requested.clone();
        if preset.harness.is_none() {
            preset.harness = crate::agents::registry::entries(&self.directory)?
                .into_iter()
                .next()
                .map(|entry| entry.id);
        }
        let _ = required_harness(&preset)?;
        preset.workspace_default = false;
        Ok(preset)
    }

    fn effective_task_preset(&self, task: &Task, pane: Option<&str>) -> Result<TaskExecutor> {
        let requested = task.executor.clone().unwrap_or(TaskExecutor {
            workspace_default: true,
            harness: None,
            model: None,
            effort: None,
            permission: None,
        });
        if !requested.workspace_default && requested.harness.is_some() {
            return Ok(requested);
        }
        if let Some(pane) = pane {
            let state = self.agent_state(pane)?;
            let harness = state.launch.id.clone();
            let model = requested.model.or_else(|| selected_option(&state, "model"));
            let effort = requested
                .effort
                .or_else(|| selected_option(&state, "effort"));
            return Ok(TaskExecutor {
                workspace_default: false,
                harness: Some(harness),
                model,
                effort,
                permission: requested.permission.or(Some(state.permission)),
            });
        }
        self.resolve_new_preset(&requested)
    }

    fn ensure_task_pane(
        &mut self,
        task: &Task,
        pane_id: &str,
        checkpoint: &DispatchCheckpoint,
    ) -> Result<bool> {
        let cwd = checkpoint
            .worktree_path
            .as_deref()
            .ok_or_else(|| errors::internal("Dispatch checkpoint has no worktree"))?;
        if self.panes.contains_key(pane_id) {
            if self.panes[pane_id].stored.instance_id.is_none()
                && self.agent_state(pane_id)?.session_id.is_none()
            {
                self.start_reserved_task_pane(task, pane_id, cwd)?;
                return Ok(true);
            }
            return self.restart_task_pane(task, pane_id, cwd);
        }
        let preset = self.effective_task_preset(task, None)?;
        let prepared = prepare_new_pane(self, task, pane_id, cwd, &preset)?;
        let saved = prepared.0.clone();
        let state = prepared.1.clone();
        let screen = prepared.2;
        let key = format!(
            "open:{}:{}:{}",
            task.workspace_id, task.task_id, task.current_attempt
        );
        let mutation = StorePort::mutate(
            &mut self.store,
            "engine-dispatch",
            &key,
            "task-pane-open",
            |change| {
                persist_new_pane(change, &saved, Some(&state))?;
                Ok((
                    json!({}),
                    vec![("pane.created".into(), json!({"pane":saved.pane}))],
                ))
            },
        )
        .map_err(errors::storage)?;
        self.publish(&mutation.events)?;
        self.panes.insert(
            pane_id.into(),
            RunningPane {
                stored: saved,
                screen,
            },
        );
        self.agents.insert(pane_id.into(), Running::new(state));
        self.spawn(pane_id)?;
        Ok(true)
    }

    fn start_reserved_task_pane(&mut self, task: &Task, pane_id: &str, cwd: &str) -> Result<()> {
        let mut saved = self
            .panes
            .get(pane_id)
            .ok_or_else(|| errors::internal("Reserved task pane disappeared"))?
            .stored
            .clone();
        let state = self.agent_state(pane_id)?;
        saved.pane.cwd = cwd.into();
        saved.pane.name = task.title.clone();
        saved.pane.state = PaneState::Starting;
        saved.pane.phrase = "Starting task session".into();
        self.commit_agent(saved, state, Changes::default(), None, json!({}))?;
        self.spawn(pane_id)?;
        Ok(())
    }

    fn restart_task_pane(&mut self, task: &Task, pane_id: &str, cwd: &str) -> Result<bool> {
        let existing = self
            .panes
            .get(pane_id)
            .ok_or_else(|| errors::internal("Task pane disappeared"))?;
        if existing.stored.pane.cwd == cwd && existing.stored.pane.name == task.title {
            return Ok(true);
        }
        let old_state = self.agent_state(pane_id)?;
        if existing.stored.pane.state != PaneState::Idle || !old_state.has_finished_turn {
            return Ok(false);
        }
        let preset = self.effective_task_preset(task, Some(pane_id))?;
        let (launch, digest) =
            crate::agents::registry::prepare(&self.directory, required_harness(&preset)?)?;
        let state = old_state.restart_for_task(launch, digest, &preset)?;
        let old_instance = existing.stored.instance_id.clone();
        let old_supervisor = existing.stored.supervisor_id.clone();
        let mut saved = existing.stored.clone();
        saved.pane.cwd = cwd.into();
        saved.pane.name = task.title.clone();
        saved.pane.state = PaneState::Starting;
        saved.pane.phrase = "Starting task session".into();
        saved.pane.generation = saved
            .pane
            .generation
            .checked_add(1)
            .ok_or_else(|| errors::internal("Pane generation exhausted"))?;
        saved.pane.pid = None;
        saved.instance_key = Uuid::new_v4().to_string();
        saved.instance_id = None;
        saved.supervisor_id = None;
        saved.sequence_base = self.panes[pane_id].screen.next_offset();
        let changes = Changes {
            retire_instance: old_instance.clone().zip(old_supervisor),
            ..Changes::default()
        };
        self.commit_agent(saved, state, changes, None, json!({}))?;
        errors::invariant(
            self.agents.get_mut(pane_id),
            "Agent runtime disappeared during task restart",
        )?
        .decoder = soyeht_protocol::ndjson::Decoder::new(0);
        if let Some(instance_id) = old_instance {
            self.pty_request(soyeht_ptyd::wire::Request::Close { instance_id })?;
        }
        self.spawn(pane_id)?;
        Ok(true)
    }

    fn accept_task_prompt(
        &mut self,
        current: Task,
        mut checkpoint: DispatchCheckpoint,
    ) -> Result<()> {
        let pane_id = checkpoint
            .pane_id
            .clone()
            .ok_or_else(|| errors::internal("Dispatch checkpoint has no pane"))?;
        let preset = self.effective_task_preset(&current, Some(&pane_id))?;
        let subitems = self
            .store
            .task_subitems(&current.workspace_id, &current.task_id)
            .map_err(errors::storage)?;
        let comments = self
            .store
            .task_comments(&current.workspace_id, &current.task_id, None, 200)
            .map_err(errors::storage)?;
        let request = worktree::DispatchRequest {
            repository: Path::new("/"),
            task: &current,
            subitems: &subitems,
            comments: &comments,
            preset,
            pane_id: pane_id.clone(),
        };
        let text = worktree::first_prompt(&request, &checkpoint)?;
        let prompt_id = format!(
            "task:{}:{}:{}",
            current.workspace_id, current.task_id, current.current_attempt
        );
        let mut state = self.agent_state(&pane_id)?;
        state.enqueue_identified(prompt_id, text, PromptOrigin::Automatic, self.timestamp())?;
        let mut updated = current.clone();
        updated.version = updated
            .version
            .checked_add(1)
            .ok_or_else(|| errors::internal("Task version exhausted"))?;
        updated.executor = Some(request.preset);
        updated.worktree = Some(TaskWorktree {
            path: checkpoint
                .worktree_path
                .clone()
                .ok_or_else(|| errors::internal("Dispatch checkpoint has no worktree"))?,
            branch: checkpoint
                .branch
                .clone()
                .ok_or_else(|| errors::internal("Dispatch checkpoint has no branch"))?,
        });
        checkpoint.stage = "prompt_accepted".into();
        let movement = self
            .store
            .task_moves(&updated.workspace_id, &updated.task_id)
            .map_err(errors::storage)?
            .into_iter()
            .last()
            .ok_or_else(|| errors::internal("Claimed task has no movement"))?;
        let saved = self
            .panes
            .get(&pane_id)
            .ok_or_else(|| errors::internal("Task pane disappeared"))?
            .stored
            .clone();
        let changes = Changes {
            events: claimed_events(&updated, &movement, &pane_id),
            task_dispatch: Some(TaskDispatchChange {
                current,
                updated,
                checkpoint,
            }),
            ..Changes::default()
        };
        self.commit_agent(saved, state, changes, None, json!({}))?;
        Ok(())
    }
}

fn prepare_new_pane(
    core: &Core,
    task: &Task,
    pane_id: &str,
    cwd: &str,
    preset: &TaskExecutor,
) -> Result<(StoredPane, State, Screen)> {
    let (launch, digest) =
        crate::agents::registry::prepare(&core.directory, required_harness(preset)?)?;
    let permission = preset.permission.clone().unwrap_or(PermissionMode::Ask);
    let mut state = State::new(launch, digest, permission)?;
    for (name, value) in [("model", &preset.model), ("effort", &preset.effort)] {
        if let Some(value) = value {
            state
                .requested_options
                .insert(name.into(), soyeht_protocol::ConfigValue::V0(value.clone()));
        }
    }
    let pane = Pane {
        pane_id: pane_id.into(),
        workspace_id: task.workspace_id.clone(),
        name: task.title.clone(),
        cwd: cwd.into(),
        mode: PaneMode::Agent,
        state: PaneState::Starting,
        source: StateSource::Protocol,
        confidence: 1.0,
        phrase: "Starting task session".into(),
        generation: 1,
        cols: 80,
        rows: 24,
        record: true,
        experiment_capture: Some(soyeht_protocol::PaneExperimentCapture {
            on: false,
            failure: None,
        }),
        pid: None,
        stream_id: None,
        agent: Some(state.public(core.now_ms())),
        url: None,
        title: None,
        viewport: None,
        incognito: None,
        writing_kind: None,
        writing_section: None,
        web_session: None,
    };
    let saved = StoredPane {
        pane,
        closed: false,
        instance_key: Uuid::new_v4().to_string(),
        instance_id: None,
        supervisor_id: None,
        sequence_base: 0,
    };
    let screen = Screen::open(&core.directory.join("sessions").join(pane_id), true)?;
    Ok((saved, state, screen))
}

fn claimed_events(task: &Task, movement: &TaskMove, pane: &str) -> Vec<(String, Value)> {
    vec![
        (
            "task.moved".into(),
            json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"actor":movement.actor,"from":movement.from,"to":movement.to,"reason":movement.reason}),
        ),
        (
            "task.claimed".into(),
            json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"paneId":pane,"attempt":task.current_attempt}),
        ),
        (
            "task.phrase_changed".into(),
            json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"phrase":task.phrase}),
        ),
        (
            "task.updated".into(),
            json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"version":task.version}),
        ),
    ]
}

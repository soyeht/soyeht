use super::{PhraseFacts, phrase, ports::TaskStorePort, review_badge};
use crate::errors::{self, Result};
use soyeht_protocol::{
    Blocking, BlockingKind, LoopConfig, LoopSetParams, Subitem, SubitemState, Task,
    TaskCompleteParams, TaskCreateParams, TaskMove, TaskState, TaskUpdateParams,
};

pub struct ExecutorIdentity<'identity> {
    pub pane_id: &'identity str,
    pub session_generation: u32,
    pub attempt: u32,
}

fn required_text(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(errors::invalid(format!("{name} must not be empty")));
    }
    Ok(())
}

pub fn new_task(
    params: &TaskCreateParams,
    task_id: String,
    actor: String,
    timestamp: String,
) -> Result<Task> {
    required_text(&params.title, "title")?;
    required_text(&params.must_not_change, "mustNotChange")?;
    let mut executor = params.executor.clone();
    if let Some(permission) = &params.permission {
        executor
            .get_or_insert(soyeht_protocol::TaskExecutor {
                workspace_default: true,
                harness: None,
                model: None,
                effort: None,
                permission: None,
            })
            .permission = Some(permission.clone());
    }
    Ok(Task {
        workspace_id: params.workspace_id.clone(),
        task_id,
        title: params.title.trim().into(),
        description: params.description.clone().unwrap_or_default(),
        how_to_verify: params.how_to_verify.clone(),
        must_not_change: params.must_not_change.trim().into(),
        state: TaskState::Todo,
        priority: params
            .priority
            .clone()
            .unwrap_or(soyeht_protocol::TaskPriority::Medium),
        planned_owner: None,
        owner_pane_id: None,
        executor,
        dependencies: params.dependencies.clone().unwrap_or_default(),
        worktree: None,
        current_attempt: 0,
        blocking: None,
        pr: None,
        pr_contract: false,
        created_at: timestamp,
        created_by: actor,
        version: 1,
        phrase: "available to claim".into(),
        review_badge: None,
        summary: None,
        contract_changes: None,
        out_of_scope_edits: None,
    })
}

pub fn validate_dependencies(
    store: &impl TaskStorePort,
    workspace: &str,
    task_id: &str,
    dependencies: &[String],
) -> Result<()> {
    for dependency in dependencies {
        if dependency == task_id
            || store
                .task(workspace, dependency)
                .map_err(errors::storage)?
                .is_none()
            || store
                .dependency_path_exists(workspace, dependency, task_id)
                .map_err(errors::storage)?
        {
            return Err(errors::dependency_cycle(format!(
                "Dependency {dependency} is outside the workspace or creates a cycle"
            )));
        }
    }
    Ok(())
}

pub fn validate_executor(
    task: &Task,
    supplied: &ExecutorIdentity<'_>,
    active_generation: u32,
) -> Result<()> {
    if task.owner_pane_id.as_deref() != Some(supplied.pane_id)
        || task.current_attempt != supplied.attempt
        || supplied.session_generation != active_generation
        || !matches!(task.state, TaskState::InProgress | TaskState::InReview)
    {
        return Err(errors::stale_attempt(
            "Mutation does not belong to the active attempt",
        ));
    }
    Ok(())
}

pub fn apply_update(task: &Task, params: &TaskUpdateParams) -> Result<Task> {
    if task.version != params.version {
        return Err(errors::stale_version("Task version changed"));
    }
    let mut updated = task.clone();
    if let Some(title) = &params.title {
        required_text(title, "title")?;
        updated.title = title.trim().into();
    }
    if let Some(description) = &params.description {
        updated.description = description.clone();
    }
    if let Some(how_to_verify) = &params.how_to_verify {
        updated.how_to_verify = Some(how_to_verify.clone());
    }
    if let Some(must_not_change) = &params.must_not_change {
        required_text(must_not_change, "mustNotChange")?;
        updated.must_not_change = must_not_change.trim().into();
    }
    if let Some(priority) = &params.priority {
        updated.priority = priority.clone();
    }
    if let Some(dependencies) = &params.dependencies {
        updated.dependencies = dependencies.clone();
        if task.state == TaskState::InProgress {
            updated.blocking = Some(Blocking {
                kind: BlockingKind::MissingInformation,
                reason: "dependency change needs confirmation".into(),
                dependency_task_id: None,
                retry_count: None,
            });
        }
    }
    if let Some(owner) = &params.planned_owner {
        updated.planned_owner = Some(owner.clone());
    }
    updated.version = updated
        .version
        .checked_add(1)
        .ok_or_else(|| errors::internal("Task version exhausted"))?;
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    Ok(updated)
}

fn drag_allowed(from: &TaskState, to: &TaskState) -> bool {
    if *to == TaskState::Cancelled && *from != TaskState::Cancelled {
        return true;
    }
    matches!(
        (from, to),
        (TaskState::InProgress, TaskState::Todo)
            | (TaskState::InProgress, TaskState::InReview)
            | (TaskState::InReview, TaskState::Completed)
            | (TaskState::InReview, TaskState::InProgress)
            | (TaskState::Cancelled, TaskState::Todo)
    )
}

pub fn apply_move(
    task: &Task,
    to: TaskState,
    actor: String,
    reason: String,
    timestamp: String,
    subitems: &[Subitem],
) -> Result<(Task, TaskMove)> {
    required_text(&reason, "reason")?;
    if to == TaskState::InProgress || !drag_allowed(&task.state, &to) {
        return Err(errors::transition_refused(format!(
            "Cannot drag a task from {:?} to {:?}",
            task.state, to
        )));
    }
    if to == TaskState::InReview && subitems.iter().any(|item| item.state == SubitemState::Open) {
        return Err(errors::transition_refused("Task has an open subitem"));
    }
    let movement = TaskMove {
        actor,
        from: task.state.clone(),
        to: to.clone(),
        reason,
        at: timestamp,
    };
    let mut updated = task.clone();
    updated.state = to;
    updated.version = next_version(task)?;
    if matches!(
        updated.state,
        TaskState::Todo | TaskState::Completed | TaskState::Cancelled
    ) {
        updated.owner_pane_id = None;
    }
    updated.review_badge = review_badge(&updated, updated.owner_pane_id.is_some());
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    Ok((updated, movement))
}

pub fn apply_block(
    task: &Task,
    kind: BlockingKind,
    reason: String,
    dependency_task_id: Option<String>,
) -> Result<Task> {
    required_text(&reason, "reason")?;
    if kind == BlockingKind::Dependency && dependency_task_id.is_none() {
        return Err(errors::dependency_open(
            "Dependency blocking needs a task id",
        ));
    }
    let retry_count = (kind == BlockingKind::ExternalError).then(|| {
        task.blocking
            .as_ref()
            .filter(|blocking| {
                blocking.kind == BlockingKind::ExternalError && blocking.reason == reason
            })
            .and_then(|blocking| blocking.retry_count)
            .unwrap_or(0)
    });
    let mut updated = task.clone();
    updated.blocking = Some(Blocking {
        kind,
        reason,
        dependency_task_id,
        retry_count,
    });
    updated.version = next_version(task)?;
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    Ok(updated)
}

pub fn apply_unblock(task: &Task) -> Result<Task> {
    let mut updated = task.clone();
    updated.blocking = None;
    updated.version = next_version(task)?;
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    Ok(updated)
}

pub fn apply_request_review(task: &Task, subitems: &[Subitem]) -> Result<Task> {
    if task.state != TaskState::InProgress {
        return Err(errors::transition_refused(
            "Only an active task can request review",
        ));
    }
    if subitems.iter().any(|item| item.state == SubitemState::Open) {
        return Err(errors::transition_refused("Task has an open subitem"));
    }
    if task.blocking.is_some() {
        return Err(errors::transition_refused(
            "A blocked task cannot request review",
        ));
    }
    let mut updated = task.clone();
    updated.state = TaskState::InReview;
    updated.version = next_version(task)?;
    updated.review_badge = Some(soyeht_protocol::TaskReviewBadge::YourTurn);
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    Ok(updated)
}

pub fn apply_complete(task: &Task, params: &TaskCompleteParams) -> Result<Task> {
    if task.state != TaskState::InReview {
        return Err(errors::transition_refused(
            "Task must be in review before completion",
        ));
    }
    required_text(&params.summary, "summary")?;
    let mut updated = task.clone();
    updated.state = TaskState::Completed;
    updated.owner_pane_id = None;
    updated.summary = Some(params.summary.clone());
    updated.contract_changes = Some(params.contract_changes.clone());
    updated.out_of_scope_edits = Some(params.out_of_scope_edits.clone());
    updated.review_badge = None;
    updated.version = next_version(task)?;
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    Ok(updated)
}

pub fn apply_review_return(task: &Task, params: &TaskCompleteParams) -> Result<Task> {
    if task.state != TaskState::InReview || task.owner_pane_id.is_none() {
        return Err(errors::transition_refused(
            "Only an owned task in review can be returned",
        ));
    }
    required_text(&params.summary, "summary")?;
    let mut updated = task.clone();
    updated.state = TaskState::InProgress;
    updated.summary = Some(params.summary.clone());
    updated.contract_changes = Some(params.contract_changes.clone());
    updated.out_of_scope_edits = Some(params.out_of_scope_edits.clone());
    updated.review_badge = Some(soyeht_protocol::TaskReviewBadge::YourTurn);
    updated.version = next_version(task)?;
    updated.phrase = "review requested changes".into();
    Ok(updated)
}

pub fn apply_release(task: &Task) -> Result<Task> {
    if !matches!(task.state, TaskState::InProgress | TaskState::InReview) {
        return Err(errors::transition_refused(
            "Only an owned task can be released",
        ));
    }
    let mut updated = task.clone();
    updated.state = TaskState::Todo;
    updated.owner_pane_id = None;
    updated.review_badge = None;
    updated.version = next_version(task)?;
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    Ok(updated)
}

pub fn apply_owner_loss(task: &Task) -> Result<Task> {
    if task.owner_pane_id.is_none()
        || !matches!(task.state, TaskState::InProgress | TaskState::InReview)
    {
        return Err(errors::transition_refused(
            "Only an owned task can lose its executor",
        ));
    }
    let mut updated = task.clone();
    if task.state == TaskState::InProgress {
        updated.state = TaskState::Todo;
    }
    updated.owner_pane_id = None;
    updated.review_badge = review_badge(&updated, false);
    updated.version = next_version(task)?;
    updated.phrase = phrase(&updated, &PhraseFacts::default());
    Ok(updated)
}

pub fn new_loop_config(params: &LoopSetParams) -> Result<LoopConfig> {
    if params.enabled {
        required_text(&params.prompt, "prompt")?;
        if params.interval == 0 {
            return Err(errors::invalid("interval must be greater than zero"));
        }
    }
    Ok(LoopConfig {
        prompt: params.prompt.clone(),
        interval: params.interval,
        max_rounds: params.max_rounds,
        stop_when: params.stop_when.clone(),
        enabled: params.enabled,
        rounds: Some(0),
        empty_results: Some(0),
    })
}

fn next_version(task: &Task) -> Result<u32> {
    task.version
        .checked_add(1)
        .ok_or_else(|| errors::internal("Task version exhausted"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{errors, tasks::ports::testing::MemoryTaskStore};
    use soyeht_protocol::{PermissionMode, TaskCreateParams, TaskPriority, TaskUpdateParams};

    pub(crate) fn task_fixture(state: TaskState) -> Task {
        Task {
            workspace_id: "workspace-1".into(),
            task_id: "WS-1".into(),
            title: "Do the work".into(),
            description: "Description".into(),
            how_to_verify: None,
            must_not_change: "Existing behavior".into(),
            state,
            priority: TaskPriority::Medium,
            planned_owner: None,
            owner_pane_id: Some("pane-1".into()),
            executor: None,
            dependencies: Vec::new(),
            worktree: None,
            current_attempt: 2,
            blocking: None,
            pr: None,
            pr_contract: false,
            created_at: "2026-09-11T00:00:00Z".into(),
            created_by: "you".into(),
            version: 3,
            phrase: String::new(),
            review_badge: None,
            summary: None,
            contract_changes: None,
            out_of_scope_edits: None,
        }
    }

    fn create_params() -> TaskCreateParams {
        TaskCreateParams {
            workspace_id: "workspace-1".into(),
            title: "Task title".into(),
            description: None,
            how_to_verify: None,
            must_not_change: "Keep the protocol stable".into(),
            executor: None,
            priority: None,
            create_worktree: None,
            permission: Some(PermissionMode::Ask),
            dependencies: None,
            dispatch: None,
            idempotency_key: "key-1".into(),
        }
    }

    #[test]
    fn create_requires_preservation_text_and_uses_canonical_defaults() {
        let task = new_task(&create_params(), "WS-1".into(), "you".into(), "now".into()).unwrap();
        assert_eq!(task.state, TaskState::Todo);
        assert_eq!(task.priority, TaskPriority::Medium);
        assert_eq!(task.phrase, "available to claim");
        assert_eq!(task.executor.unwrap().permission, Some(PermissionMode::Ask));

        let mut invalid = create_params();
        invalid.must_not_change = "  ".into();
        assert_eq!(
            errors::kind(
                &new_task(&invalid, "WS-2".into(), "you".into(), "now".into()).unwrap_err()
            ),
            soyeht_protocol::ErrorKind::InvalidParams
        );
    }

    #[test]
    fn dependencies_reject_self_missing_cross_workspace_and_cycles() {
        let mut store = MemoryTaskStore::default();
        store.tasks.insert(
            ("workspace-1".into(), "WS-2".into()),
            task_fixture(TaskState::Todo),
        );
        store
            .edges
            .insert(("workspace-1".into(), "WS-2".into()), vec!["WS-1".into()]);
        for dependencies in [
            vec!["WS-1".into()],
            vec!["WS-9".into()],
            vec!["WS-2".into()],
        ] {
            let error =
                validate_dependencies(&store, "workspace-1", "WS-1", &dependencies).unwrap_err();
            assert_eq!(
                errors::kind(&error),
                soyeht_protocol::ErrorKind::DependencyCycle
            );
        }
    }

    #[test]
    fn executor_mutations_require_the_current_pane_generation_and_attempt() {
        let task = task_fixture(TaskState::InProgress);
        let current = ExecutorIdentity {
            pane_id: "pane-1",
            session_generation: 8,
            attempt: 2,
        };
        validate_executor(&task, &current, 8).unwrap();
        let stale = ExecutorIdentity {
            attempt: 1,
            ..current
        };
        let error = validate_executor(&task, &stale, 8).unwrap_err();
        assert_eq!(
            errors::kind(&error),
            soyeht_protocol::ErrorKind::StaleAttempt
        );
    }

    #[test]
    fn update_requires_the_current_version_and_blocks_dependency_changes_while_active() {
        let task = task_fixture(TaskState::InProgress);
        let params = TaskUpdateParams {
            workspace_id: task.workspace_id.clone(),
            task_id: task.task_id.clone(),
            version: task.version,
            title: None,
            description: None,
            how_to_verify: None,
            must_not_change: None,
            priority: Some(TaskPriority::High),
            dependencies: Some(vec!["WS-2".into()]),
            planned_owner: None,
            idempotency_key: "key".into(),
        };
        let updated = apply_update(&task, &params).unwrap();
        assert_eq!(updated.version, 4);
        assert_eq!(updated.priority, TaskPriority::High);
        assert_eq!(
            updated.blocking.unwrap().kind,
            BlockingKind::MissingInformation
        );

        let mut stale = params;
        stale.version = 2;
        assert_eq!(
            errors::kind(&apply_update(&task, &stale).unwrap_err()),
            soyeht_protocol::ErrorKind::StaleVersion
        );
    }

    #[test]
    fn dragging_refuses_in_progress_and_todo_to_completed_and_records_accepted_moves() {
        let todo = task_fixture(TaskState::Todo);
        for target in [TaskState::InProgress, TaskState::Completed] {
            let error = apply_move(
                &todo,
                target,
                "you".into(),
                "drag".into(),
                "now".into(),
                &[],
            )
            .unwrap_err();
            assert_eq!(
                errors::kind(&error),
                soyeht_protocol::ErrorKind::TransitionRefused
            );
        }
        let active = task_fixture(TaskState::InProgress);
        let (moved, history) = apply_move(
            &active,
            TaskState::Todo,
            "you".into(),
            "returned".into(),
            "now".into(),
            &[],
        )
        .unwrap();
        assert_eq!(moved.state, TaskState::Todo);
        assert_eq!(history.from, TaskState::InProgress);
        assert_eq!(history.to, TaskState::Todo);
        assert_eq!(history.reason, "returned");
    }

    #[test]
    fn review_waits_for_every_subitem_and_completion_preserves_both_declaration_lists() {
        let active = task_fixture(TaskState::InProgress);
        let open = Subitem {
            subitem_id: "WS-1-T01".into(),
            task_id: active.task_id.clone(),
            text: "First step".into(),
            agent: None,
            instructions: None,
            state: SubitemState::Open,
        };
        assert!(apply_request_review(&active, std::slice::from_ref(&open)).is_err());
        let mut done = open;
        done.state = SubitemState::Done;
        let review = apply_request_review(&active, &[done]).unwrap();
        let params: TaskCompleteParams = serde_json::from_value(serde_json::json!({
            "workspaceId":"workspace-1","taskId":"WS-1","summary":"done",
            "contractChanges":[],"outOfScopeEdits":[],"paneId":"pane-1",
            "sessionGeneration":8,"attempt":2,"idempotencyKey":"key"
        }))
        .unwrap();
        let completed = apply_complete(&review, &params).unwrap();
        assert_eq!(completed.state, TaskState::Completed);
        assert_eq!(completed.contract_changes, Some(Vec::new()));
        assert_eq!(completed.out_of_scope_edits, Some(Vec::new()));
    }

    #[test]
    fn returned_review_preserves_the_attempt_owner_and_declared_completion_inputs() {
        let review = task_fixture(TaskState::InReview);
        let params = TaskCompleteParams {
            workspace_id: review.workspace_id.clone(),
            task_id: review.task_id.clone(),
            summary: "Review candidate".into(),
            contract_changes: Vec::new(),
            out_of_scope_edits: Vec::new(),
            pane_id: review.owner_pane_id.clone(),
            session_generation: Some(1),
            attempt: Some(review.current_attempt),
            idempotency_key: "complete".into(),
        };

        let returned = apply_review_return(&review, &params).unwrap();

        assert_eq!(returned.state, TaskState::InProgress);
        assert_eq!(returned.owner_pane_id, review.owner_pane_id);
        assert_eq!(returned.current_attempt, review.current_attempt);
        assert_eq!(returned.summary.as_deref(), Some("Review candidate"));
        assert_eq!(returned.contract_changes, Some(Vec::new()));
        assert_eq!(returned.out_of_scope_edits, Some(Vec::new()));
        assert_eq!(
            returned.review_badge,
            Some(soyeht_protocol::TaskReviewBadge::YourTurn)
        );
    }

    #[test]
    fn typed_blocking_requires_a_dependency_identity_and_release_unbinds_the_attempt() {
        let active = task_fixture(TaskState::InProgress);
        let error =
            apply_block(&active, BlockingKind::Dependency, "waiting".into(), None).unwrap_err();
        assert_eq!(
            errors::kind(&error),
            soyeht_protocol::ErrorKind::DependencyOpen
        );

        let blocked = apply_block(
            &active,
            BlockingKind::Dependency,
            "waiting for WS-2".into(),
            Some("WS-2".into()),
        )
        .unwrap();
        assert_eq!(blocked.phrase, "blocked: waiting for WS-2");
        let released = apply_release(&blocked).unwrap();
        assert_eq!(released.state, TaskState::Todo);
        assert_eq!(released.owner_pane_id, None);
    }

    #[test]
    fn owner_loss_returns_active_work_but_keeps_review_waiting_for_a_person() {
        let active = task_fixture(TaskState::InProgress);
        let returned = apply_owner_loss(&active).unwrap();
        assert_eq!(returned.state, TaskState::Todo);
        assert_eq!(returned.owner_pane_id, None);

        let review = task_fixture(TaskState::InReview);
        let waiting = apply_owner_loss(&review).unwrap();
        assert_eq!(waiting.state, TaskState::InReview);
        assert_eq!(waiting.owner_pane_id, None);
        assert_eq!(
            waiting.review_badge,
            Some(soyeht_protocol::TaskReviewBadge::YourTurn)
        );
    }

    #[test]
    fn loop_configuration_counts_only_enabled_nonempty_intervals() {
        let params: LoopSetParams = serde_json::from_value(serde_json::json!({
            "paneId":"pane-1","prompt":"pull the next task","interval":60,
            "maxRounds":10,"stopWhen":"three_empty","enabled":true,
            "idempotencyKey":"loop-1"
        }))
        .unwrap();
        let config = new_loop_config(&params).unwrap();
        assert_eq!(config.rounds, Some(0));
        assert_eq!(config.empty_results, Some(0));

        let mut invalid = params;
        invalid.interval = 0;
        assert_eq!(
            errors::kind(&new_loop_config(&invalid).unwrap_err()),
            soyeht_protocol::ErrorKind::InvalidParams
        );
    }
}

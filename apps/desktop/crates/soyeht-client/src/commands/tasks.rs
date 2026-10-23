//! The requests of tasks: reading, creating, moving and dispatching them.

use super::{Command, key};
use soyeht_protocol::{
    Request, TaskCreateParams, TaskDispatchParams, TaskMoveParams, TaskShowParams,
};

/// The request a command of a task needs, or `None` when it is not one of these. All the tasks of
/// a workspace come in pages, which the engine fetches one after the other.
pub(super) fn request(command: &Command) -> Option<Request> {
    Some(match command {
        Command::LoadTask {
            workspace_id,
            task_id,
        } => Request::TaskShow(TaskShowParams {
            workspace_id: workspace_id.clone(),
            task_id: task_id.clone(),
        }),
        Command::CreateTask { task, .. } => Request::TaskCreate(TaskCreateParams {
            workspace_id: task.workspace_id.clone(),
            title: task.title.clone(),
            description: Some(task.description.clone()),
            how_to_verify: task.how_to_verify.clone(),
            must_not_change: task.must_not_change.clone(),
            executor: task.executor.clone(),
            priority: Some(task.priority.clone()),
            create_worktree: Some(task.create_worktree),
            permission_mode: None,
            dependencies: Some(task.dependencies.clone()),
            dispatch: Some(task.dispatch),
            idempotency_key: key(),
        }),
        Command::MoveTask {
            workspace_id,
            task_id,
            state,
            reason,
        } => Request::TaskMove(TaskMoveParams {
            workspace_id: workspace_id.clone(),
            task_id: task_id.clone(),
            state: state.clone(),
            reason: reason.clone(),
            idempotency_key: key(),
        }),
        Command::DispatchTask {
            workspace_id,
            task_id,
            preset,
            ..
        } => Request::TaskDispatch(TaskDispatchParams {
            workspace_id: workspace_id.clone(),
            task_id: task_id.clone(),
            preset: preset.clone(),
            idempotency_key: key(),
        }),
        _ => return None,
    })
}

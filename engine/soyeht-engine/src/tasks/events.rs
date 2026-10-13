//! The notifications every task change repeats: the move, the new version and the new phrase.

use serde_json::{Value, json};
use soyeht_protocol::{Task, TaskMove, event};

pub(super) fn moved(task: &Task, movement: &TaskMove) -> (String, Value) {
    (
        event::TASK_MOVED.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"actor":movement.actor,"from":movement.from,"to":movement.to,"reason":movement.reason}),
    )
}

pub(super) fn updated(task: &Task) -> (String, Value) {
    (
        event::TASK_UPDATED.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"version":task.version}),
    )
}

pub(super) fn phrase_changed(task: &Task) -> (String, Value) {
    (
        event::TASK_PHRASE_CHANGED.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"phrase":task.phrase}),
    )
}

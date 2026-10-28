//! The notifications every task change repeats. Every task event is built here, so the shape
//! of a payload lives in one place.

use serde_json::{Value, json};
use soyeht_protocol::{AttentionItem, BlockingKind, Comment, Subitem, Task, TaskMove, event};

pub(super) fn created(task: &Task) -> (String, Value) {
    (event::TASK_CREATED.into(), json!({"task":task}))
}

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

/// The new version, then the new phrase when the change moved it.
pub(super) fn revised(task: &Task, phrase_moved: bool) -> Vec<(String, Value)> {
    let mut events = vec![updated(task)];
    if phrase_moved {
        events.push(phrase_changed(task));
    }
    events
}

pub(super) fn commented(task: &Task, comment: &Comment) -> (String, Value) {
    (
        event::TASK_COMMENTED.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"comment":comment}),
    )
}

pub(super) fn subitem_changed(task: &Task, subitem: &Subitem) -> (String, Value) {
    (
        event::TASK_SUBITEM_CHANGED.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"subitem":subitem}),
    )
}

pub(super) fn claimed(task: &Task, pane: &str) -> (String, Value) {
    (
        event::TASK_CLAIMED.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"paneId":pane,"attempt":task.current_attempt}),
    )
}

pub(super) fn released(task: &Task, reason: &str, attempt: u32) -> (String, Value) {
    (
        event::TASK_RELEASED.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"reason":reason,"attempt":attempt}),
    )
}

pub(super) fn blocked(task: &Task, kind: &BlockingKind, reason: &str) -> (String, Value) {
    (
        event::TASK_BLOCKED.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"kind":kind,"reason":reason}),
    )
}

pub(super) fn unblocked(task: &Task, reason: &str) -> (String, Value) {
    (
        event::TASK_UNBLOCKED.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"reason":reason}),
    )
}

pub(super) fn attention(task: &Task) -> (String, Value) {
    (
        event::TASK_ATTENTION.into(),
        json!({"workspaceId":task.workspace_id,"taskId":task.task_id,"n":1}),
    )
}

/// The attention list of a workspace after a task change added or removed an item.
pub(super) fn attention_changed(workspace: &str, items: &[AttentionItem]) -> (String, Value) {
    (
        event::ATTENTION_CHANGED.into(),
        json!({"workspaceId":workspace,"count":items.len(),"items":items}),
    )
}

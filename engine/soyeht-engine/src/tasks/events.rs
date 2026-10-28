//! The notifications every task change repeats. Every task event is built here, so the shape
//! of a payload lives in one place.

use serde_json::{Value, json};
use soyeht_protocol::{BlockingKind, Comment, Subitem, Task, TaskMove, event};

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

/// The payloads below were read off the inline `json!` calls that these builders replaced, and
/// are kept as literals so a later change to a builder cannot move the wire unnoticed.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::rules::tests::task_fixture;
    use soyeht_protocol::{SubitemState, TaskState};

    fn task() -> Task {
        let mut task = task_fixture(TaskState::InProgress);
        task.phrase = "Waiting on review".into();
        task
    }

    fn comment() -> Comment {
        Comment {
            comment_id: "comment-1".into(),
            task_id: "WS-1".into(),
            author: "you".into(),
            text: "Looks right".into(),
            at: "2026-09-11T00:00:01Z".into(),
        }
    }

    #[test]
    fn created_carries_the_whole_task() {
        let task = task();
        let (name, payload) = created(&task);
        assert_eq!(name, "task.created");
        assert_eq!(payload["task"], serde_json::to_value(&task).unwrap());
        assert_eq!(payload.as_object().unwrap().len(), 1);
    }

    #[test]
    fn moved_names_the_actor_the_states_and_the_reason() {
        let movement = TaskMove {
            actor: "you".into(),
            from: TaskState::Todo,
            to: TaskState::InProgress,
            reason: "started".into(),
            at: "2026-09-11T00:00:01Z".into(),
        };
        assert_eq!(
            moved(&task(), &movement),
            (
                "task.moved".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1","actor":"you",
                    "from":"todo","to":"in_progress","reason":"started"})
            )
        );
    }

    #[test]
    fn updated_and_phrase_changed_carry_the_version_and_the_phrase() {
        assert_eq!(
            updated(&task()),
            (
                "task.updated".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1","version":3})
            )
        );
        assert_eq!(
            phrase_changed(&task()),
            (
                "task.phrase_changed".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1","phrase":"Waiting on review"})
            )
        );
    }

    #[test]
    fn revised_adds_the_phrase_only_when_it_moved() {
        assert_eq!(revised(&task(), false), vec![updated(&task())]);
        assert_eq!(
            revised(&task(), true),
            vec![updated(&task()), phrase_changed(&task())]
        );
    }

    #[test]
    fn commented_carries_the_comment() {
        assert_eq!(
            commented(&task(), &comment()),
            (
                "task.commented".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1","comment":{
                    "commentId":"comment-1","taskId":"WS-1","author":"you",
                    "text":"Looks right","at":"2026-09-11T00:00:01Z"}})
            )
        );
    }

    #[test]
    fn subitem_changed_carries_the_subitem() {
        let subitem = Subitem {
            subitem_id: "sub-1".into(),
            task_id: "WS-1".into(),
            text: "Write the test".into(),
            agent: None,
            instructions: None,
            state: SubitemState::Done,
        };
        assert_eq!(
            subitem_changed(&task(), &subitem),
            (
                "task.subitem_changed".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1","subitem":{
                    "subitemId":"sub-1","taskId":"WS-1","text":"Write the test","state":"done"}})
            )
        );
    }

    #[test]
    fn claimed_names_the_pane_and_the_attempt() {
        assert_eq!(
            claimed(&task(), "pane-1"),
            (
                "task.claimed".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1","paneId":"pane-1","attempt":2})
            )
        );
    }

    #[test]
    fn released_gives_the_reason_and_the_attempt() {
        assert_eq!(
            released(&task(), "returned by executor", 2),
            (
                "task.released".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1",
                    "reason":"returned by executor","attempt":2})
            )
        );
    }

    #[test]
    fn blocked_and_unblocked_give_the_kind_and_the_reason() {
        assert_eq!(
            blocked(
                &task(),
                &BlockingKind::MissingInformation,
                "need the schema"
            ),
            (
                "task.blocked".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1",
                    "kind":"missing_information","reason":"need the schema"})
            )
        );
        assert_eq!(
            unblocked(&task(), "unblocked"),
            (
                "task.unblocked".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1","reason":"unblocked"})
            )
        );
    }

    #[test]
    fn attention_counts_one() {
        assert_eq!(
            attention(&task()),
            (
                "task.attention".into(),
                json!({"workspaceId":"workspace-1","taskId":"WS-1","n":1})
            )
        );
    }
}

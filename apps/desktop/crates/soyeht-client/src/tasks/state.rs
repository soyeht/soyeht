//! The replica of a workspace's tasks: the board, the detail of each task opened, and what needs
//! the person's attention.

use crate::state::{Effect, State};
use serde_json::Value;
use soyeht_protocol::{
    AttentionChangedEvent, AttentionItem, EventName, Task, TaskCommentedEvent, TaskCreatedEvent,
    TaskMovedEvent, TaskPhraseChangedEvent, TaskShowResult, TaskSubitemChangedEvent,
};

/// What the engine told the client about a task, to apply to the replica.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskUpdate {
    /// All the tasks of one workspace.
    Workspace {
        workspace_id: String,
        tasks: Vec<Task>,
    },
    /// One task with its subitems, comments, attempts and history.
    Detail(Box<TaskShowResult>),
    /// A task a request the person made created or dispatched.
    Changed(Task),
    /// A task a request the person made moved: read again, as the engine may have changed more.
    Moved(Task),
}

impl State {
    pub fn tasks_in<'a>(&'a self, workspace_id: &'a str) -> impl Iterator<Item = &'a Task> {
        self.tasks
            .iter()
            .filter(move |task| task.workspace_id == workspace_id)
    }

    pub fn task(&self, task_id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.task_id == task_id)
    }

    pub fn task_detail(&self, task_id: &str) -> Option<&TaskShowResult> {
        self.task_details.get(task_id)
    }

    pub fn attention_in(&self, workspace_id: &str) -> &[AttentionItem] {
        self.attention.get(workspace_id).map_or(&[], Vec::as_slice)
    }

    /// The tasks and the attention items a snapshot lists. Attention belongs to the workspace of
    /// the pane it is about.
    pub(crate) fn adopt_tasks(&mut self, tasks: &[Value], attention: &[Value]) {
        match decode_all::<Task>(tasks) {
            Ok(items) => self.tasks = items,
            Err(error) => self.report(
                crate::state::problem::kind::TASKS_UNREADABLE,
                format!("The tasks could not be read ({error})."),
            ),
        }
        let known: Vec<String> = self.tasks.iter().map(|task| task.task_id.clone()).collect();
        self.task_details.retain(|id, _| known.contains(id));
        self.attention.clear();
        match decode_all::<AttentionItem>(attention) {
            Ok(items) => {
                for item in items {
                    let workspace = self
                        .panes
                        .iter()
                        .find(|pane| pane.pane_id == item.pane_id)
                        .map(|pane| pane.workspace_id.clone());
                    if let Some(workspace) = workspace {
                        self.attention.entry(workspace).or_default().push(item);
                    }
                }
            }
            Err(error) => {
                self.report(
                    crate::state::problem::kind::ATTENTION_UNREADABLE,
                    format!("What needs you could not be read ({error})."),
                );
            }
        }
    }

    pub(crate) fn apply_task_update(&mut self, update: TaskUpdate) -> Vec<Effect> {
        match update {
            TaskUpdate::Workspace {
                workspace_id,
                tasks,
            } => {
                self.tasks.retain(|task| task.workspace_id != workspace_id);
                self.tasks.extend(tasks);
                Vec::new()
            }
            TaskUpdate::Detail(detail) => {
                self.upsert_task(detail.task.clone());
                self.task_details
                    .insert(detail.task.task_id.clone(), *detail);
                Vec::new()
            }
            TaskUpdate::Changed(task) => {
                self.upsert_task(task);
                Vec::new()
            }
            TaskUpdate::Moved(task) => {
                let reload = Effect::LoadTask {
                    workspace_id: task.workspace_id.clone(),
                    task_id: task.task_id.clone(),
                };
                self.upsert_task(task);
                vec![reload]
            }
        }
    }

    /// A task replaces the one with its key and goes to the end, as on the Mac.
    pub(crate) fn upsert_task(&mut self, task: Task) {
        self.tasks.retain(|known| known.task_id != task.task_id);
        if let Some(detail) = self.task_details.get_mut(&task.task_id) {
            detail.task = task.clone();
        }
        self.tasks.push(task);
    }

    pub(crate) fn task_notification(
        &mut self,
        name: EventName,
        params: Value,
    ) -> serde_json::Result<Vec<Effect>> {
        Ok(match name {
            EventName::TaskCreated => {
                self.upsert_task(serde_json::from_value::<TaskCreatedEvent>(params)?.task);
                Vec::new()
            }
            EventName::TaskPhraseChanged => {
                let event = serde_json::from_value::<TaskPhraseChangedEvent>(params)?;
                self.change_task(&event.task_id, |task| task.phrase = event.phrase.clone());
                Vec::new()
            }
            EventName::TaskCommented => {
                let event = serde_json::from_value::<TaskCommentedEvent>(params)?;
                if let Some(detail) = self.task_details.get_mut(&event.task_id) {
                    detail
                        .comments
                        .retain(|known| known.comment_id != event.comment.comment_id);
                    detail.comments.push(event.comment);
                }
                Vec::new()
            }
            EventName::TaskSubitemChanged => {
                let event = serde_json::from_value::<TaskSubitemChangedEvent>(params)?;
                if let Some(detail) = self.task_details.get_mut(&event.task_id) {
                    detail
                        .subitems
                        .retain(|known| known.subitem_id != event.subitem.subitem_id);
                    detail.subitems.push(event.subitem);
                }
                Vec::new()
            }
            // What changed in a task is told by the task itself: read it again.
            EventName::TaskMoved => {
                let event = serde_json::from_value::<TaskMovedEvent>(params)?;
                self.reload_task(event.workspace_id, event.task_id)
            }
            EventName::TaskClaimed
            | EventName::TaskReleased
            | EventName::TaskBlocked
            | EventName::TaskUnblocked
            | EventName::TaskUpdated => {
                let reference = (
                    params.get("workspaceId").and_then(Value::as_str),
                    params.get("taskId").and_then(Value::as_str),
                );
                match reference {
                    (Some(workspace), Some(task)) => {
                        self.reload_task(workspace.to_owned(), task.to_owned())
                    }
                    _ => Vec::new(),
                }
            }
            EventName::AttentionChanged => {
                let event = serde_json::from_value::<AttentionChangedEvent>(params)?;
                self.attention.insert(event.workspace_id, event.items);
                Vec::new()
            }
            _ => Vec::new(),
        })
    }

    fn reload_task(&self, workspace_id: String, task_id: String) -> Vec<Effect> {
        if self.connection == crate::state::Connection::Connected {
            vec![Effect::LoadTask {
                workspace_id,
                task_id,
            }]
        } else {
            Vec::new()
        }
    }

    fn change_task(&mut self, task_id: &str, change: impl Fn(&mut Task)) {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.task_id == task_id) {
            change(task);
            let task = task.clone();
            if let Some(detail) = self.task_details.get_mut(task_id) {
                detail.task = task;
            }
        }
    }
}

fn decode_all<T: serde::de::DeserializeOwned>(items: &[Value]) -> serde_json::Result<Vec<T>> {
    items
        .iter()
        .map(|item| serde_json::from_value(item.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        Event, Snapshot,
        state::{Effect, State},
        tasks::TaskUpdate,
        testing::{event, pane, workspace},
    };
    use serde_json::{Value, json};
    use soyeht_protocol::EventName;

    fn task(id: &str, state: &str, phrase: &str) -> Value {
        json!({"workspaceId": "w", "taskId": id, "title": id, "description": "d", "mustNotChange": "m",
               "state": state, "priority": "medium", "dependencies": [], "currentAttempt": 0,
               "prContract": false, "createdAt": "2026-10-09T20:00:00Z", "createdBy": "you",
               "version": 1, "phrase": phrase})
    }

    fn connected() -> State {
        let mut state = State::default();
        state.apply(Event::Connected(Snapshot {
            panes: vec![pane("a", "w", "agent")],
            workspaces: vec![workspace("w")],
            tasks: vec![task("WS-1", "todo", "free"), task("WS-2", "todo", "free")],
            attention: vec![
                json!({"attentionId": "x1", "paneId": "a", "message": "Needs an answer", "createdAt": "now"}),
                json!({"attentionId": "x2", "paneId": "gone", "message": "Lost", "createdAt": "now"}),
            ],
            ..Snapshot::default()
        }));
        state
    }

    fn ids(state: &State) -> Vec<&str> {
        state
            .tasks_in("w")
            .map(|task| task.task_id.as_str())
            .collect()
    }

    #[test]
    fn a_snapshot_brings_the_tasks_and_what_needs_the_person_by_workspace() {
        let state = connected();
        assert_eq!(ids(&state), ["WS-1", "WS-2"]);
        assert_eq!(state.attention_in("w").len(), 1);
        assert!(state.attention_in("other").is_empty() && state.problem.is_none());
    }

    #[test]
    fn a_task_that_changes_goes_to_the_end_and_its_phrase_follows_events() {
        let mut state = connected();
        state.apply(event(
            EventName::TaskCreated,
            json!({"task": task("WS-3", "todo", "new")}),
        ));
        state.apply(event(
            EventName::TaskCreated,
            json!({"task": task("WS-1", "in_progress", "busy")}),
        ));
        assert_eq!(ids(&state), ["WS-2", "WS-3", "WS-1"]);
        state.apply(event(
            EventName::TaskPhraseChanged,
            json!({"workspaceId": "w", "taskId": "WS-2", "phrase": "waiting"}),
        ));
        assert_eq!(state.task("WS-2").unwrap().phrase, "waiting");
    }

    #[test]
    fn what_an_event_does_not_carry_is_read_again_from_the_engine() {
        let mut state = connected();
        let reload = |id: &str| Effect::LoadTask {
            workspace_id: "w".into(),
            task_id: id.into(),
        };
        let moved = state.apply(event(
            EventName::TaskMoved,
            json!({"workspaceId": "w", "taskId": "WS-1", "actor": "a", "from": "todo", "to": "in_progress", "reason": "r"}),
        ));
        assert_eq!(moved, [reload("WS-1")]);
        let blocked = state.apply(event(
            EventName::TaskBlocked,
            json!({"workspaceId": "w", "taskId": "WS-2"}),
        ));
        assert_eq!(blocked, [reload("WS-2")]);
        let moved_by_me = state.apply(Event::Task(TaskUpdate::Moved(
            serde_json::from_value(task("WS-1", "in_review", "ci")).unwrap(),
        )));
        assert_eq!(moved_by_me, [reload("WS-1")]);
        state.apply(Event::Disconnected("gone".into()));
        let offline = state.apply(event(
            EventName::TaskUpdated,
            json!({"workspaceId": "w", "taskId": "WS-1", "version": 2}),
        ));
        assert!(offline.is_empty());
    }

    #[test]
    fn a_detail_keeps_its_comments_and_subitems_as_events_change_them() {
        let mut state = connected();
        let detail = json!({"task": task("WS-1", "todo", "free"), "subitems": [], "comments": [],
                            "attempts": [], "history": []});
        state.apply(Event::Task(TaskUpdate::Detail(Box::new(
            serde_json::from_value(detail).unwrap(),
        ))));
        let comment = |text: &str| {
            json!({"workspaceId": "w", "taskId": "WS-1", "comment": {"commentId": "c1", "taskId": "WS-1",
                   "author": "you", "text": text, "at": "now"}})
        };
        state.apply(event(EventName::TaskCommented, comment("first")));
        state.apply(event(EventName::TaskCommented, comment("edited")));
        let held = state.task_detail("WS-1").unwrap();
        assert_eq!(held.comments.len(), 1);
        assert_eq!(held.comments[0].text, "edited");
        state.apply(event(
            EventName::TaskSubitemChanged,
            json!({"workspaceId": "w", "taskId": "WS-1", "subitem": {"subitemId": "s1", "taskId": "WS-1",
                   "text": "step", "state": "done"}}),
        ));
        assert_eq!(state.task_detail("WS-1").unwrap().subitems.len(), 1);
        state.apply(event(
            EventName::AttentionChanged,
            json!({"workspaceId": "w", "count": 0, "items": []}),
        ));
        assert!(state.attention_in("w").is_empty());
    }

    #[test]
    fn a_dispatch_settles_the_pane_it_opened_and_a_workspace_load_replaces_its_tasks() {
        let mut state = connected();
        state.expect_open(7, "w");
        let dispatched: soyeht_protocol::Task =
            serde_json::from_value(task("WS-1", "in_progress", "started")).unwrap();
        let effects = state.apply(Event::TaskPane {
            task: dispatched,
            ticket: 7,
            pane_id: Some("a".into()),
        });
        assert_eq!(effects, [Effect::FocusPane("a".into())]);
        state.apply(Event::Task(TaskUpdate::Workspace {
            workspace_id: "w".into(),
            tasks: vec![serde_json::from_value(task("WS-9", "todo", "x")).unwrap()],
        }));
        assert_eq!(ids(&state), ["WS-9"]);
    }
}

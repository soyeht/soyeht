//! The workspace's tasks surface: which view is shown, which task is open, the new-task form, and
//! what the person's clicks ask of the engine. What the surface says comes from
//! `soyeht_client::tasks`.

use crate::{
    composer_field::{ComposerEvent, ComposerField, new_field},
    destination::Destination,
    workbench::Workbench,
};
use gpui::{Context, Entity};
use soyeht_client::{
    Command,
    commands::NewTask,
    tasks::{self, PRIORITIES},
};
use soyeht_protocol::{TaskExecutor, TaskPriority, TaskState, client_policy};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TaskView {
    #[default]
    Board,
    List,
}

/// The option of the new-task form whose choices are listed under the options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewChoice {
    Runner,
    Priority,
    Worktree,
    Dependencies,
}

/// A task being written: what the person has typed and chosen, until it is created.
pub(crate) struct NewTaskForm {
    pub title: Entity<ComposerField>,
    pub description: Entity<ComposerField>,
    pub must_not_change: Entity<ComposerField>,
    pub priority: TaskPriority,
    pub create_worktree: bool,
    /// Run it with the workspace's default agent, else with `harness`.
    pub workspace_default: bool,
    pub harness: Option<String>,
    pub dependencies: BTreeSet<String>,
    pub choice: Option<NewChoice>,
    pub submitting: bool,
}

#[derive(Default)]
pub(crate) struct TasksUi {
    pub view: TaskView,
    /// The task whose detail is open.
    pub selected: Option<String>,
    pub form: Option<NewTaskForm>,
    /// The list of what needs the person is open.
    pub attention: bool,
    /// The Dispatch list is open.
    pub dispatch_menu: bool,
}

impl Workbench {
    /// The workspace the tasks surface shows: the selected one.
    pub(crate) fn tasks_workspace(&self) -> Option<String> {
        self.state.selected_workspace_id()
    }

    /// Opens the tasks surface for the selected workspace and reads its tasks again.
    pub(crate) fn open_tasks(&mut self, cx: &mut Context<Self>) {
        self.select_destination(Destination::Tasks, cx);
    }

    /// Reads the workspace's tasks when the surface is shown, as the Mac does.
    pub(crate) fn refresh_tasks(&mut self) {
        if let Some(workspace_id) = self.tasks_workspace() {
            self.send(Command::LoadTasks { workspace_id });
        }
    }

    pub(crate) fn set_task_view(&mut self, view: TaskView, cx: &mut Context<Self>) {
        self.tasks_ui.view = view;
        cx.notify();
    }

    pub(crate) fn open_task(&mut self, task_id: &str, cx: &mut Context<Self>) {
        if let Some(task) = self.state.task(task_id) {
            let (workspace_id, task_id) = (task.workspace_id.clone(), task.task_id.clone());
            self.send(Command::LoadTask {
                workspace_id,
                task_id: task_id.clone(),
            });
            self.tasks_ui.selected = Some(task_id);
            cx.notify();
        }
    }

    pub(crate) fn close_task(&mut self, cx: &mut Context<Self>) {
        self.tasks_ui.selected = None;
        cx.notify();
    }

    pub(crate) fn toggle_attention(&mut self, cx: &mut Context<Self>) {
        self.tasks_ui.attention = !self.tasks_ui.attention;
        self.tasks_ui.dispatch_menu = false;
        cx.notify();
    }

    pub(crate) fn toggle_dispatch_menu(&mut self, cx: &mut Context<Self>) {
        self.tasks_ui.dispatch_menu = !self.tasks_ui.dispatch_menu;
        self.tasks_ui.attention = false;
        cx.notify();
    }

    /// A card was dropped on a column: the task moves there.
    pub(crate) fn move_task(&mut self, task_id: &str, state: TaskState, cx: &mut Context<Self>) {
        let Some(task) = self.state.task(task_id) else {
            return;
        };
        if task.state == state {
            return;
        }
        let workspace_id = task.workspace_id.clone();
        self.send(Command::MoveTask {
            workspace_id,
            task_id: task_id.to_owned(),
            state,
            reason: client_policy::TASK_MOVE_REASON.to_owned(),
        });
        cx.notify();
    }

    /// Hands a task to an agent in a new pane, which takes the keyboard when it arrives.
    pub(crate) fn dispatch_task(&mut self, task_id: &str, cx: &mut Context<Self>) {
        let Some(task) = self.state.task(task_id) else {
            return;
        };
        let (workspace_id, preset) = (
            task.workspace_id.clone(),
            task.executor.clone().unwrap_or(TaskExecutor {
                workspace_default: true,
                harness: None,
                model: None,
                effort: None,
                permission_mode: None,
            }),
        );
        let ticket = self.next_ticket();
        self.state.expect_open(ticket, &workspace_id);
        let taken = self.send(Command::DispatchTask {
            workspace_id,
            task_id: task_id.to_owned(),
            preset,
            ticket,
        });
        if !taken {
            self.state
                .apply(soyeht_client::Event::OpenFailed { ticket });
        }
        self.tasks_ui.dispatch_menu = false;
        cx.notify();
    }

    pub(crate) fn open_new_task(&mut self, cx: &mut Context<Self>) {
        let palette = self.theme.palette();
        let field = |cx: &mut Context<Self>| {
            let field = new_field(palette, cx);
            cx.subscribe(
                &field,
                |this, field, event: &ComposerEvent, cx| match event {
                    // Return adds a line in the text areas and creates nothing by itself.
                    ComposerEvent::Submit => field.update(cx, |field, cx| field.insert("\n", cx)),
                    ComposerEvent::Dismiss => this.close_new_task(cx),
                },
            )
            .detach();
            field
        };
        let form = NewTaskForm {
            title: field(cx),
            description: field(cx),
            must_not_change: field(cx),
            priority: TaskPriority::Medium,
            create_worktree: true,
            workspace_default: true,
            harness: None,
            dependencies: BTreeSet::new(),
            choice: None,
            submitting: false,
        };
        self.tasks_ui.form = Some(form);
        cx.notify();
    }

    pub(crate) fn close_new_task(&mut self, cx: &mut Context<Self>) {
        self.tasks_ui.form = None;
        cx.notify();
    }

    pub(crate) fn edit_new_task(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut NewTaskForm),
    ) {
        if let Some(form) = self.tasks_ui.form.as_mut() {
            change(form);
            cx.notify();
        }
    }

    /// The text of the form's fields: the title, the description and what must not change.
    pub(crate) fn new_task_text(&self, cx: &mut Context<Self>) -> Option<[String; 3]> {
        let form = self.tasks_ui.form.as_ref()?;
        let read = |field: &Entity<ComposerField>, cx: &mut Context<Self>| {
            field.read(cx).draft().text().to_owned()
        };
        Some([
            read(&form.title, cx),
            read(&form.description, cx),
            read(&form.must_not_change, cx),
        ])
    }

    /// Creates the task, and dispatches it when asked, once the required fields are filled.
    pub(crate) fn submit_new_task(&mut self, dispatch: bool, cx: &mut Context<Self>) {
        let Some([title, description, must_not_change]) = self.new_task_text(cx) else {
            return;
        };
        let Some(workspace_id) = self.tasks_workspace() else {
            return;
        };
        let Some(form) = self.tasks_ui.form.as_mut() else {
            return;
        };
        let missing = tasks::missing_required_fields(&title, &description, &must_not_change);
        if form.submitting
            || !missing.is_empty()
            || (!form.workspace_default && form.harness.is_none())
        {
            return;
        }
        form.submitting = true;
        let executor = TaskExecutor {
            workspace_default: form.workspace_default,
            harness: form.harness.clone().filter(|_| !form.workspace_default),
            model: None,
            effort: None,
            permission_mode: None,
        };
        let task = NewTask {
            workspace_id: workspace_id.clone(),
            title,
            description,
            how_to_verify: None,
            must_not_change,
            executor: Some(executor),
            priority: form.priority.clone(),
            create_worktree: form.create_worktree,
            dependencies: form.dependencies.iter().cloned().collect(),
            dispatch,
        };
        let ticket = self.next_ticket();
        if dispatch {
            self.state.expect_open(ticket, &workspace_id);
        }
        if self.send(Command::CreateTask {
            task: Box::new(task),
            ticket,
        }) {
            self.tasks_ui.form = None;
        } else {
            self.state
                .apply(soyeht_client::Event::OpenFailed { ticket });
            self.edit_new_task(cx, |form| form.submitting = false);
        }
        cx.notify();
    }

    pub(crate) fn priorities() -> &'static [TaskPriority] {
        &PRIORITIES
    }
}

//! The board: a column for each state and a flat card for each task, which can be dragged to
//! another column.

use super::chips::{
    CARD_GLYPH, comment_count, count_pill, muted_mono, owner_chip, priority_tag, status_row,
    task_title,
};
use crate::{
    theme::{Palette, desktop, pane_interior, space, type_scale},
    views::prelude::*,
};
use gpui::{AppContext, Render, Window};
use soyeht_client::tasks::{self};
use soyeht_protocol::{Task, TaskState};

/// A card on its way to another column: the key of the task.
#[derive(Clone)]
pub(super) struct TaskDrag {
    pub task_id: String,
    pub palette: &'static Palette,
}

impl Render for TaskDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(space::X10))
            .py(px(space::X6))
            .rounded(px(pane_interior::CARD_RADIUS))
            .bg(rgb(self.palette.canvas))
            .text_color(rgb(self.palette.ink))
            .mono()
            .font_weight(FontWeight::BOLD)
            .text_size(px(type_scale::SMALL))
            .child(SharedString::from(self.task_id.clone()))
    }
}

impl Workbench {
    pub(super) fn board(
        &mut self,
        tasks: &[Task],
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let refs: Vec<&Task> = tasks.iter().collect();
        let columns: Vec<AnyElement> = tasks::board_states(&refs)
            .into_iter()
            .map(|state| {
                let cards: Vec<&Task> = refs.iter().copied().filter(|t| t.state == state).collect();
                self.column(state, &cards, palette, cx)
            })
            .collect();
        row()
            .id("task-board")
            .items_start()
            .gap(px(space::X16))
            .flex_1()
            .min_h_0()
            .overflow_scroll()
            .children(columns)
            .into_any_element()
    }

    fn column(
        &mut self,
        state: TaskState,
        tasks: &[&Task],
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = state.clone();
        let header = row()
            .gap(px(space::X8))
            .px(px(space::X4))
            .chrome()
            .font_weight(FontWeight::BOLD)
            .text_size(px(type_scale::CONTROL))
            .text_color(rgb(palette.secondary))
            .child(SharedString::from(
                tasks::state_title(&state).to_uppercase(),
            ))
            .child(count_pill(palette, tasks.len()));
        let cards: Vec<AnyElement> = tasks
            .iter()
            .map(|task| self.card(task, palette, cx))
            .collect();
        column()
            .id(id("task-column", tasks::state_title(&state)))
            .flex_none()
            .w(px(desktop::TASK_COLUMN_WIDTH))
            .min_h(px(desktop::TASK_COLUMN_MIN_HEIGHT))
            .gap(px(space::X10))
            .child(header)
            .children(cards)
            .drag_over::<TaskDrag>(move |style, _, _, _| {
                style
                    .bg(rgb(palette.well))
                    .rounded(px(pane_interior::CARD_RADIUS))
            })
            .on_drop(cx.listener(move |this, drag: &TaskDrag, _, cx| {
                this.move_task(&drag.task_id, target.clone(), cx)
            }))
            .into_any_element()
    }

    /// One task: its key and priority, its title, where it stands, who has it and its branch.
    fn card(
        &mut self,
        task: &Task,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (owner, comments) = self.task_facts(task);
        let (opened, dragged) = (task.task_id.clone(), task.task_id.clone());
        column()
            .id(id("task-card", &task.task_id))
            .gap(px(space::X7))
            .px(px(space::X12))
            .py(px(space::X10))
            .rounded(px(pane_interior::CARD_RADIUS))
            .bg(rgb(palette.canvas))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_task(&opened, cx)))
            .on_drag(
                TaskDrag {
                    task_id: dragged,
                    palette,
                },
                |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .child(
                row()
                    .gap(px(space::X6))
                    .child(muted_mono(task.task_id.clone(), palette))
                    .child(div().flex_1())
                    .child(priority_tag(&task.priority, palette)),
            )
            .child(task_title(task, palette))
            .child(status_row(
                tasks::indicator(task),
                tasks::status_line(task),
                desktop::TASK_DOT,
                Some(2),
                palette,
            ))
            .child(
                row()
                    .gap(px(space::X8))
                    .text_size(px(type_scale::MICRO))
                    .text_color(rgb(palette.muted))
                    .children(owner.map(|name| owner_chip(&name, palette)))
                    .children(task.worktree.as_ref().map(|worktree| {
                        row()
                            .gap(px(space::X5))
                            .mono()
                            .min_w_0()
                            .child(icon(Icon::Branch, CARD_GLYPH, palette.muted))
                            .child(
                                div()
                                    .truncate()
                                    .child(SharedString::from(worktree.branch.clone())),
                            )
                    }))
                    .child(div().flex_1())
                    .child(comment_count(comments, palette)),
            )
            .when(
                self.tasks_ui.selected.as_deref() == Some(task.task_id.as_str()),
                |card| card.bg(rgb(palette.well)),
            )
            .into_any_element()
    }

    /// The name of the pane that owns a task and how many comments it has.
    pub(super) fn task_facts(&self, task: &Task) -> (Option<String>, usize) {
        let comments = self
            .state
            .task_detail(&task.task_id)
            .map_or(0, |detail| detail.comments.len());
        (self.owner_name(task), comments)
    }

    /// The name of the pane that owns a task, or its key when the pane is gone.
    pub(super) fn owner_name(&self, task: &Task) -> Option<String> {
        let owner = task.owner_pane_id.as_ref()?;
        Some(
            self.state
                .pane(owner)
                .map_or_else(|| owner.clone(), |pane| pane.name.clone()),
        )
    }
}

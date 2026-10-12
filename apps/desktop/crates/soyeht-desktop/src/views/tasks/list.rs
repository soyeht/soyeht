//! The list: a table of the workspace's tasks, grouped by state, the most urgent first.

use super::chips::{
    comment_count, count_pill, muted_mono, owner_chip, priority_chip, state_chip, status_row,
    task_title,
};
use crate::{
    theme::{Palette, desktop, space, type_scale},
    views::{
        prelude::*,
        ui::{mono_note, rule},
    },
};
use soyeht_client::tasks;
use soyeht_protocol::Task;

/// A column of the table: its heading and how wide it is (the task's own column fills).
const COLUMNS: [(&str, Option<f32>); 8] = [
    ("KEY", Some(desktop::TASK_LIST_KEY_WIDTH)),
    ("TASK", None),
    ("STATE", Some(desktop::TASK_LIST_STATE_WIDTH)),
    ("PRIORITY", Some(desktop::TASK_LIST_PRIORITY_WIDTH)),
    ("OWNER", Some(desktop::TASK_LIST_OWNER_WIDTH)),
    ("WORKTREE", Some(desktop::TASK_LIST_WORKTREE_WIDTH)),
    ("", Some(desktop::TASK_LIST_COMMENTS_WIDTH)),
    ("UPDATED", Some(desktop::TASK_LIST_UPDATED_WIDTH)),
];

/// A cell of the table in its column.
fn cell(width: Option<f32>) -> Div {
    match width {
        Some(width) => div().w(px(width)).flex_none(),
        None => div()
            .flex_1()
            .min_w(px(desktop::TASK_LIST_TITLE_MINIMUM_WIDTH)),
    }
}

impl Workbench {
    pub(super) fn list(
        &mut self,
        cx: &mut Context<Self>,
        tasks: &[Task],
        palette: &'static Palette,
    ) -> AnyElement {
        let refs: Vec<&Task> = tasks.iter().collect();
        let mut groups: Vec<AnyElement> = Vec::new();
        for state in tasks::list_states(&refs) {
            let mut rows: Vec<&Task> = refs.iter().copied().filter(|t| t.state == state).collect();
            if rows.is_empty() {
                continue;
            }
            rows.sort_by(|left, right| tasks::list_order(left, right));
            let count = rows.len();
            let lines: Vec<AnyElement> = rows
                .into_iter()
                .map(|task| self.list_row(task, palette, cx))
                .collect();
            groups.push(
                column()
                    .child(
                        row()
                            .gap(px(space::X8))
                            .pl(px(space::X8))
                            .pr(px(space::X12))
                            .pt(px(space::X8))
                            .pb(px(space::X3))
                            .chrome()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(type_scale::SMALL))
                            .text_color(rgb(palette.muted))
                            .child(icon(
                                Icon::ChevronDown,
                                desktop::DISCLOSURE_GLYPH,
                                palette.muted,
                            ))
                            .child(SharedString::from(
                                tasks::state_title(&state).to_uppercase(),
                            ))
                            .child(count_pill(palette, count)),
                    )
                    .children(lines)
                    .into_any_element(),
            );
        }
        let header = row()
            .gap(px(space::X12))
            .px(px(space::X12))
            .pb(px(space::X6))
            .chrome()
            .font_weight(FontWeight::BOLD)
            .text_size(px(type_scale::MICRO))
            .text_color(rgb(palette.muted))
            .children(COLUMNS.map(|(name, width)| cell(width).child(name)));
        column()
            .id("task-list")
            .flex_1()
            .min_h_0()
            .min_w(px(desktop::TASK_LIST_MINIMUM_WIDTH))
            .overflow_scroll()
            .child(header)
            .child(rule(palette.well, desktop::HAIRLINE))
            .children(groups)
            .into_any_element()
    }

    fn list_row(
        &mut self,
        task: &Task,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let opened = task.task_id.clone();
        let (owner, comments) = self.task_facts(task);
        column()
            .child(
                row()
                    .id(id("task-row", &task.task_id))
                    .gap(px(space::X12))
                    .px(px(space::X12))
                    .py(px(space::X5))
                    .rounded(px(space::X10))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(palette.well)))
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.open_task(&opened, cx)),
                    )
                    .child(cell(COLUMNS[0].1).child(muted_mono(task.task_id.clone(), palette)))
                    .child(
                        cell(COLUMNS[1].1).child(
                            column()
                                .gap(px(space::X3))
                                .child(task_title(task, palette).truncate())
                                .child(status_row(
                                    tasks::state_role(&task.state),
                                    task.phrase.clone(),
                                    space::X5,
                                    None,
                                    palette,
                                )),
                        ),
                    )
                    .child(cell(COLUMNS[2].1).child(state_chip(&task.state, palette)))
                    .child(cell(COLUMNS[3].1).child(priority_chip(&task.priority, palette)))
                    .child(cell(COLUMNS[4].1).child(match owner {
                        Some(name) => owner_chip(&name, palette),
                        None => dash(palette),
                    }))
                    .child(
                        cell(COLUMNS[5].1).child(
                            mono_note(palette).child(SharedString::from(
                                task.worktree
                                    .as_ref()
                                    .map_or("\u{2014}".to_owned(), |tree| tree.branch.clone()),
                            )),
                        ),
                    )
                    .child(cell(COLUMNS[6].1).child(comment_count(comments, palette)))
                    .child(cell(COLUMNS[7].1).child(dash(palette))),
            )
            .child(
                div()
                    .mx(px(space::X12))
                    .child(rule(palette.well, desktop::HAIRLINE)),
            )
            .into_any_element()
    }
}

/// A dash where a cell has nothing to say.
fn dash(palette: &Palette) -> Div {
    div()
        .mono()
        .text_size(px(type_scale::SMALL))
        .text_color(rgb(palette.muted))
        .child("\u{2014}")
}

//! The detail of a task: a panel at the side of the surface with what the engine knows of it.

use super::chips::{capsule, owner_chip, priority_chip, role_color, state_chip, well_card};
use crate::{
    neo,
    theme::{Palette, desktop, elevation, radius, space, type_scale},
    views::prelude::*,
};
use soyeht_client::tasks;
use soyeht_protocol::Task;
use soyeht_protocol::{Attempt, Subitem, SubitemState};

/// A section of the detail: a small heading over its content.
fn group(palette: &Palette, heading: impl Into<SharedString>, content: Div) -> Div {
    column()
        .gap(px(space::X8))
        .child(
            chrome_text(FontWeight::BOLD, type_scale::MICRO)
                .text_color(rgb(palette.secondary))
                .child(SharedString::from(heading.into().to_uppercase())),
        )
        .child(content)
}

fn text_block(text: impl Into<SharedString>, color: u32) -> Div {
    div()
        .text_size(px(type_scale::CONTROL))
        .text_color(rgb(color))
        .child(text.into())
}

/// A list of declarations in a well, or "None declared."
fn declarations(palette: &Palette, title: &str, values: Vec<String>) -> Div {
    well_card(palette)
        .flex()
        .flex_col()
        .gap(px(space::X5))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(type_scale::SMALL))
                .child(SharedString::from(title.to_owned())),
        )
        .children(if values.is_empty() {
            vec![text_block("None declared.", palette.secondary).text_size(px(type_scale::SMALL))]
        } else {
            values
                .into_iter()
                .map(|value| {
                    div()
                        .mono()
                        .text_size(px(type_scale::SMALL))
                        .child(SharedString::from(value))
                })
                .collect()
        })
}

impl Workbench {
    /// The panel of the open task, or none when no task is open.
    pub(super) fn task_sheet(
        &mut self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let task_id = self.tasks_ui.selected.clone()?;
        let task = self.state.task(&task_id).cloned();
        let body = match &task {
            Some(task) => self.task_content(task, palette),
            None => text_block("Loading\u{2026}", palette.secondary),
        };
        Some(
            neo::raised(palette, radius::CARD, elevation::PANEL)
                .id(id("task-sheet", &task_id))
                .absolute()
                .top(px(space::X8))
                .right(px(space::X12))
                .bottom(px(space::X12))
                .w(px(desktop::TASK_SHEET_WIDTH))
                .max_h(px(desktop::TASK_SHEET_HEIGHT))
                .flex()
                .flex_col()
                .gap(px(space::X12))
                .px(px(space::X18 + space::X2))
                .py(px(space::X18))
                .occlude()
                .child(
                    row()
                        .gap(px(space::X8))
                        .child(
                            div()
                                .mono()
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(type_scale::CONTROL))
                                .text_color(rgb(palette.muted))
                                .child(SharedString::from(task_id.clone())),
                        )
                        .child(spacer())
                        .child(
                            row()
                                .id(id("task-sheet-close", &task_id))
                                .cursor_pointer()
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.close_task(cx)),
                                )
                                .child(icon(
                                    Icon::Close,
                                    desktop::HEADER_ACTION_GLYPH,
                                    palette.muted,
                                )),
                        ),
                )
                .child(
                    div()
                        .id(id("task-sheet-scroll", &task_id))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn task_content(&self, task: &Task, palette: &'static Palette) -> Div {
        let detail = self.state.task_detail(&task.task_id);
        let comments = detail.map_or_else(Vec::new, |detail| detail.comments.clone());
        let timeline = detail.map(tasks::timeline).unwrap_or_default();
        column()
            .gap(px(space::X12))
            .child(self.task_head(task, palette))
            .child(group(
                palette,
                "Description",
                text_block(task.description.clone(), palette.ink),
            ))
            .child(group(
                palette,
                "Must not change",
                well_card(palette).child(text_block(task.must_not_change.clone(), palette.ink)),
            ))
            .children(task.how_to_verify.as_ref().map(|how| {
                group(
                    palette,
                    "How to verify",
                    text_block(how.clone(), palette.ink),
                )
            }))
            .children(task.blocking.as_ref().map(|blocking| {
                group(
                    palette,
                    tasks::blocking_heading(blocking),
                    text_block(blocking.reason.clone(), palette.ink),
                )
            }))
            .child(subitems_group(
                detail.map_or(&[], |detail| detail.subitems.as_slice()),
                palette,
            ))
            .child(attempts_group(
                detail.map_or(&[], |detail| detail.attempts.as_slice()),
                palette,
            ))
            .child(group(
                palette,
                "Summary",
                match &task.summary {
                    Some(summary) => text_block(summary.clone(), palette.ink),
                    None => text_block("No summary yet.", palette.secondary),
                },
            ))
            .child(group(
                palette,
                "Drift review",
                self.drift(task, &comments, palette),
            ))
            .child(timeline_group(timeline, palette))
    }

    /// The title, the state, priority, owner and review badge, and who runs the task.
    fn task_head(&self, task: &Task, palette: &'static Palette) -> Div {
        let owner = self.owner_name(task);
        let facts = row()
            .gap(px(space::X8))
            .flex_wrap()
            .child(state_chip(&task.state, palette))
            .child(priority_chip(&task.priority, palette))
            .children(owner.map(|name| owner_chip(&name, palette)))
            .children(task.review_badge.as_ref().map(|badge| {
                capsule(
                    palette.well,
                    tasks::badge_title(badge),
                    role_color(tasks::badge_role(badge), palette),
                )
            }));
        let runner = row()
            .gap(px(space::X8))
            .child(
                row()
                    .gap(px(space::X6))
                    .px(px(space::X10))
                    .py(px(space::X4))
                    .rounded_full()
                    .bg(rgb(palette.well))
                    .chrome()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(type_scale::SMALL))
                    .child(icon(Icon::Cpu, desktop::INLINE_GLYPH, palette.ink))
                    .child(SharedString::from(tasks::runner_name(
                        task.executor.as_ref(),
                    ))),
            )
            .children(task.worktree.as_ref().map(|worktree| {
                div()
                    .mono()
                    .truncate()
                    .text_size(px(type_scale::SMALL))
                    .text_color(rgb(palette.muted))
                    .child(SharedString::from(format!(
                        "{} \u{b7} attempt {}",
                        worktree.branch, task.current_attempt
                    )))
            }));
        column()
            .gap(px(space::X12))
            .child(
                chrome_text(FontWeight::BOLD, type_scale::HEADING)
                    .child(SharedString::from(task.title.clone())),
            )
            .child(facts)
            .child(runner)
    }

    /// What the reviewer found: the contract changes and edits out of scope the task declares,
    /// and the lines of the comments that name a change.
    fn drift(&self, task: &Task, comments: &[soyeht_protocol::Comment], palette: &Palette) -> Div {
        let changes = task
            .contract_changes
            .iter()
            .flatten()
            .map(tasks::contract_change)
            .collect();
        let edits = task
            .out_of_scope_edits
            .iter()
            .flatten()
            .map(tasks::out_of_scope_edit)
            .collect();
        let findings = tasks::drift_findings(comments);
        column()
            .gap(px(space::X8))
            .child(declarations(palette, "Contract changes", changes))
            .child(declarations(palette, "Out-of-scope edits", edits))
            .children(if findings.is_empty() {
                vec![text_block("No reviewer findings.", palette.secondary)]
            } else {
                findings
                    .into_iter()
                    .map(|line| {
                        div()
                            .mono()
                            .text_size(px(type_scale::SMALL))
                            .child(SharedString::from(line))
                    })
                    .collect()
            })
    }
}

/// The subitems, each with a mark that says whether it is done, and who has it.
fn subitems_group(subitems: &[Subitem], palette: &Palette) -> Div {
    let lines: Vec<Div> = if subitems.is_empty() {
        vec![text_block("No subitems.", palette.secondary)]
    } else {
        subitems
            .iter()
            .map(|item| {
                let done = item.state == SubitemState::Done;
                row()
                    .gap(px(space::X8))
                    .text_size(px(type_scale::CAPTION))
                    .text_color(rgb(if done { palette.muted } else { palette.ink }))
                    .child(icon(
                        if done { Icon::Check } else { Icon::Circle },
                        desktop::INLINE_GLYPH,
                        if done {
                            palette.success()
                        } else {
                            palette.muted
                        },
                    ))
                    .child(div().flex_1().child(SharedString::from(item.text.clone())))
                    .children(item.agent.clone().map(|agent| {
                        div()
                            .text_size(px(type_scale::SMALL))
                            .text_color(rgb(palette.muted))
                            .child(SharedString::from(agent))
                    }))
            })
            .collect()
    };
    group(
        palette,
        tasks::subitems_title(subitems),
        column().gap(px(space::X8)).children(lines),
    )
}

/// The attempts to do the task: who tried, how it went and what they said.
fn attempts_group(attempts: &[Attempt], palette: &Palette) -> Div {
    group(
        palette,
        "Attempts",
        column()
            .gap(px(space::X8))
            .children(attempts.iter().map(|attempt| {
                column()
                    .gap(px(space::X3))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(type_scale::CONTROL))
                            .child(SharedString::from(tasks::attempt_line(attempt))),
                    )
                    .child(
                        text_block(tasks::attempt_outcome(attempt), palette.secondary)
                            .text_size(px(type_scale::SMALL)),
                    )
                    .children(attempt.summary.clone().map(|summary| {
                        text_block(summary, palette.ink).text_size(px(type_scale::SMALL))
                    }))
            })),
    )
}

/// The moves between states and the comments, oldest first, each with its author.
fn timeline_group(entries: Vec<tasks::TimelineEntry>, palette: &Palette) -> Div {
    group(
        palette,
        "History and comments",
        column()
            .gap(px(space::X6))
            .children(entries.into_iter().map(|entry| {
                column()
                    .gap(px(space::X2))
                    .children(
                        entry
                            .author
                            .as_deref()
                            .map(|author| owner_chip(author, palette)),
                    )
                    .child(
                        text_block(
                            entry.text.clone(),
                            if entry.author.is_none() {
                                palette.secondary
                            } else {
                                palette.ink
                            },
                        )
                        .font_weight(FontWeight::MEDIUM),
                    )
                    .child(
                        div()
                            .mono()
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(type_scale::MICRO))
                            .text_color(rgb(palette.muted))
                            .child(SharedString::from(entry.at)),
                    )
            })),
    )
}

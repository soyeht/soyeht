//! The workspace's tasks, as the Mac's task screen draws them: a pane card with a header
//! capsule, a toolbar, and the board or the list; a panel with the detail of the task opened,
//! another for a new task, and the lists the header and the Dispatch button open.

mod board;
mod chips;
mod detail;
mod list;
mod new_task;

use self::chips::well_card;
use crate::{
    neo,
    tasks_ui::TaskView,
    theme::{Palette, desktop, elevation, pane_layout, radius, space, type_scale},
    views::{
        prelude::*,
        ui::{empty_state, mono_label, option_pill},
    },
};
use soyeht_client::{Connection, tasks};
use soyeht_protocol::Task;

/// The raised panel of the lists the header's summary and the Dispatch button open.
fn popover(palette: &'static Palette, width: f32) -> Div {
    neo::raised(palette, radius::CARD, elevation::PANEL)
        .absolute()
        .w(px(width))
        .flex()
        .flex_col()
        .gap(px(space::X10))
        .p(px(space::X14))
        .occlude()
}

impl Workbench {
    /// The tasks surface of the selected workspace.
    pub(in crate::views) fn tasks_surface(
        &mut self,
        workspace_id: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let held: Vec<Task> = self.state.tasks_in(workspace_id).cloned().collect();
        let name = self.state.workspace(workspace_id).map_or_else(
            || "Workspace".to_owned(),
            |workspace| workspace.name.clone(),
        );
        let attention = self.state.attention_in(workspace_id).len();
        let summary = tasks::summary(&held.iter().collect::<Vec<_>>(), attention);
        let connected = self.state.connection == Connection::Connected;
        let view = self.tasks_ui.view;
        let content = if held.is_empty() {
            self.tasks_empty(palette, connected, cx)
        } else if view == TaskView::Board {
            self.board(&held, palette, cx)
        } else {
            self.list(cx, &held, palette)
        };
        let header = self.tasks_header(&name, &summary, view, palette, cx);
        let toolbar = self.tasks_toolbar(view, connected, palette, cx);
        let popovers = [
            self.attention_popover(workspace_id, palette, cx),
            self.dispatch_popover(&held, palette, cx),
            self.task_sheet(palette, cx),
            self.new_task_sheet(palette, cx),
        ];
        neo::pane_shell(palette)
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(space::X8))
            .p(px(pane_layout::PANE_CARD_INSET))
            .child(header)
            .child(
                column()
                    .flex_1()
                    .min_h_0()
                    .gap(px(space::X14))
                    .px(px(space::X16))
                    .py(px(space::X14))
                    .child(toolbar)
                    .child(content),
            )
            .children(popovers.into_iter().flatten())
            .into_any_element()
    }

    /// "tasks · Soyeht CI", what needs the person, the board and the list buttons.
    fn tasks_header(
        &mut self,
        name: &str,
        summary: &str,
        view: TaskView,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let button = |key: &str,
                      glyph: Icon,
                      selected: bool,
                      view: Option<TaskView>,
                      cx: &mut Context<Self>| {
            row()
                .id(id("tasks-view", key))
                .cursor_pointer()
                .own_press()
                .child(icon(
                    glyph,
                    desktop::HEADER_ACTION_GLYPH,
                    if selected {
                        palette.ink
                    } else {
                        palette.secondary
                    },
                ))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| match view {
                    Some(view) => this.set_task_view(view, cx),
                    None => this.select_destination(crate::destination::Destination::Agents, cx),
                }))
        };
        row()
            .gap(px(space::X7))
            .px(px(space::X12))
            .py(px(space::X6))
            .rounded_full()
            .bg(rgb(palette.well))
            .text_color(rgb(palette.secondary))
            .child(icon(
                Icon::ListChecks,
                desktop::HEADER_ACTION_GLYPH,
                palette.secondary,
            ))
            .child(mono_label().child(SharedString::from(format!("tasks \u{b7} {name}"))))
            .child(spacer())
            .child(
                div()
                    .id("tasks-summary")
                    .cursor_pointer()
                    .own_press()
                    .mono()
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(px(type_scale::SMALL))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_attention(cx)))
                    .child(SharedString::from(summary.to_owned())),
            )
            .child(button(
                "board",
                Icon::Columns,
                view == TaskView::Board,
                Some(TaskView::Board),
                cx,
            ))
            .child(button(
                "list",
                Icon::Rows,
                view == TaskView::List,
                Some(TaskView::List),
                cx,
            ))
            .child(button("close", Icon::Close, false, None, cx))
    }

    /// The Board and List toggle, Dispatch and New task.
    fn tasks_toolbar(
        &mut self,
        view: TaskView,
        connected: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let choice = |key: &'static str,
                      glyph: Icon,
                      text: &'static str,
                      which: TaskView,
                      cx: &mut Context<Self>| {
            let selected = view == which;
            let frame = if selected {
                neo::raised(palette, radius::PILL, elevation::TOGGLE)
            } else {
                div()
            };
            frame
                .id(id("task-view-choice", key))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(space::X6))
                .px(px(space::X12))
                .py(px(space::X6))
                .chrome()
                .font_weight(if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::SEMIBOLD
                })
                .text_size(px(type_scale::CONTROL))
                .text_color(rgb(if selected { palette.ink } else { palette.muted }))
                .cursor_pointer()
                .own_press()
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.set_task_view(which, cx)),
                )
                .child(icon(
                    glyph,
                    desktop::INLINE_GLYPH,
                    if selected { palette.ink } else { palette.muted },
                ))
                .child(text)
        };
        row()
            .gap(px(space::X12))
            .min_h(px(desktop::TASK_TOOLBAR_HEIGHT))
            .child(
                row()
                    .gap(px(space::X2))
                    .p(px(space::X3))
                    .rounded_full()
                    .bg(rgb(palette.well))
                    .child(choice("board", Icon::Grid, "Board", TaskView::Board, cx))
                    .child(choice("list", Icon::List, "List", TaskView::List, cx)),
            )
            .child(spacer())
            .child(
                option_pill(
                    palette,
                    "task-dispatch",
                    Icon::Send,
                    "Dispatch",
                    self.tasks_ui.dispatch_menu,
                )
                .when(!connected, |pill| {
                    pill.opacity(crate::theme::pane_interior::DISABLED_OPACITY)
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if connected {
                        this.toggle_dispatch_menu(cx)
                    }
                })),
            )
            .child(self.new_task_button(connected, palette, cx))
    }

    /// The accent pill that opens the new-task form.
    fn new_task_button(
        &mut self,
        connected: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("task-new")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(space::X5))
            .px(px(space::X10))
            .py(px(space::X6))
            .rounded_full()
            .bg(rgb(palette.accent))
            .chrome()
            .font_weight(FontWeight::BOLD)
            .text_size(px(type_scale::CONTROL))
            .text_color(rgb(palette.on_accent))
            .when(!connected, |pill| {
                pill.opacity(crate::theme::pane_interior::DISABLED_OPACITY)
            })
            .when(connected, |pill| pill.cursor_pointer())
            .own_press()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if connected {
                    this.open_new_task(cx)
                }
            }))
            .child(icon(Icon::Plus, desktop::INLINE_GLYPH, palette.on_accent))
            .child("New task")
            .into_any_element()
    }

    /// What stands where the board would be while the workspace has no tasks.
    fn tasks_empty(
        &mut self,
        palette: &'static Palette,
        connected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        empty_state(
            palette,
            Some(Icon::ListChecks),
            tasks::EMPTY_TITLE,
            Some(tasks::EMPTY_DETAIL.into()),
        )
        .gap(px(space::X14))
        .child(self.new_task_button(connected, palette, cx))
        .into_any_element()
    }

    /// Who needs the person, each with a way to the pane.
    fn attention_popover(
        &mut self,
        workspace_id: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.tasks_ui.attention {
            return None;
        }
        let items = self.state.attention_in(workspace_id).to_vec();
        let rows: Vec<AnyElement> = items
            .iter()
            .map(|item| {
                let pane_id = item.pane_id.clone();
                well_card(palette)
                    .flex()
                    .flex_col()
                    .gap(px(space::X8))
                    .child(
                        div()
                            .text_size(px(type_scale::CONTROL))
                            .child(SharedString::from(item.message.clone())),
                    )
                    .child(
                        div()
                            .id(id("attention-go", &item.attention_id))
                            .cursor_pointer()
                            .own_press()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(type_scale::CONTROL))
                            .text_color(rgb(palette.accent))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.tasks_ui.attention = false;
                                this.select_destination(
                                    crate::destination::Destination::Agents,
                                    cx,
                                );
                                this.focus_pane(&pane_id, window, cx);
                            }))
                            .child("Go to pane"),
                    )
                    .into_any_element()
            })
            .collect();
        Some(
            popover(palette, desktop::ATTENTION_POPOVER_WIDTH)
                .id("attention-popover")
                .top(px(space::X18 * 2.0))
                .right(px(space::X18))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.tasks_ui.attention = false;
                    cx.notify();
                }))
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(type_scale::BODY))
                        .child(SharedString::from(format!("{} need you", items.len()))),
                )
                .children(if rows.is_empty() {
                    vec![
                        div()
                            .text_color(rgb(palette.secondary))
                            .child("Nothing needs your attention.")
                            .into_any_element(),
                    ]
                } else {
                    rows
                })
                .into_any_element(),
        )
    }

    /// The tasks that are ready to hand to an agent.
    fn dispatch_popover(
        &mut self,
        held: &[Task],
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.tasks_ui.dispatch_menu {
            return None;
        }
        let refs: Vec<&Task> = held.iter().collect();
        let ready = tasks::ready_to_dispatch(&refs);
        let rows: Vec<AnyElement> = ready
            .iter()
            .map(|task| {
                let task_id = task.task_id.clone();
                crate::views::menus::on_pick(
                    crate::views::menus::item(
                        palette,
                        &task.task_id,
                        tasks::dispatch_label(task),
                        false,
                    ),
                    cx,
                    move |this, cx| this.dispatch_task(&task_id, cx),
                )
            })
            .collect();
        Some(
            popover(palette, desktop::ATTENTION_POPOVER_WIDTH)
                .id("dispatch-popover")
                .top(px(space::X18 * 3.0))
                .right(px(space::X18 * 4.0))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.tasks_ui.dispatch_menu = false;
                    cx.notify();
                }))
                .children(if rows.is_empty() {
                    vec![
                        crate::views::menus::note(palette, tasks::NO_TASK_READY).into_any_element(),
                    ]
                } else {
                    rows
                })
                .into_any_element(),
        )
    }
}

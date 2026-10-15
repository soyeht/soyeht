//! The new-task form: a title, a description, what must not change, who runs it and a few
//! options, in a panel over the surface.

use crate::composer_field::ComposerField;
use crate::{
    neo,
    tasks_ui::NewChoice,
    theme::{Palette, desktop, elevation, pane_interior, radius, space, type_scale},
    views::{
        menus,
        prelude::*,
        ui::{option_pill, switch},
    },
};
use gpui::Entity;
use soyeht_client::tasks;
use soyeht_protocol::TaskPriority;

/// The wording of the form.
const TITLE: &str = "New task";
const SUBTITLE: &str =
    "It enters this workspace list. An agent can claim it, or you can dispatch it.";
const WHO_WILL_RUN: &str = "WHO WILL RUN";
const USE_DEFAULT: &str = "Use workspace default";
const CREATE_IN_LIST: &str = "Create in list";
const CREATE_AND_DISPATCH: &str = "Create and dispatch";
#[cfg(target_os = "macos")]
const SHORTCUTS: &str = "\u{2318}\u{21a9} create \u{b7} \u{21e7}\u{2318}\u{21a9} create & dispatch";
#[cfg(not(target_os = "macos"))]
const SHORTCUTS: &str = "Ctrl+Enter create \u{b7} Ctrl+Shift+Enter create & dispatch";

fn label(palette: &Palette, text: &str) -> gpui::Div {
    chrome_text(FontWeight::BOLD, type_scale::SMALL)
        .text_color(rgb(palette.muted))
        .child(SharedString::from(text.to_owned()))
}

/// A text area in a well, over its label.
fn editor(
    palette: &Palette,
    title: &str,
    field: &Entity<crate::composer_field::ComposerField>,
    height: f32,
) -> gpui::Div {
    column()
        .gap(px(space::X8))
        .child(label(palette, title))
        .child(
            neo::well(palette, radius::CONTROL, desktop::WELL_DEPTH)
                .h(px(height))
                .px(px(space::X10))
                .py(px(space::X8))
                .overflow_hidden()
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(type_scale::CAPTION))
                .child(field.clone()),
        )
}

impl Workbench {
    pub(super) fn new_task_sheet(
        &mut self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let [title, description, must_not_change] = self.new_task_text(cx)?;
        let form = self.tasks_ui.form.as_ref()?;
        let missing = tasks::missing_required_fields(&title, &description, &must_not_change);
        let harness_name = form
            .harness
            .as_ref()
            .and_then(|harness| self.state.harness_name(harness))
            .map(str::to_owned);
        let valid = missing.is_empty() && (form.workspace_default || form.harness.is_some());
        let hint = tasks::action_hint(
            &missing,
            form.workspace_default,
            form.harness.as_deref().unwrap_or_default(),
            SHORTCUTS,
        );
        let submitting = form.submitting;
        let (priority, worktree, choice) =
            (form.priority.clone(), form.create_worktree, form.choice);
        let dependencies: Vec<String> = form.dependencies.iter().cloned().collect();
        let runner =
            tasks::runner_title(form.workspace_default, harness_name.as_deref(), None, None);
        let default_runner = form.workspace_default;
        let fields = (
            form.title.clone(),
            form.description.clone(),
            form.must_not_change.clone(),
        );
        let choices = self.new_task_choices(choice, palette, cx);
        let options = [
            (NewChoice::Priority, Icon::Hash, priority_label(&priority)),
            (
                NewChoice::Worktree,
                Icon::Branch,
                if worktree { "worktree" } else { "workspace" }.to_owned(),
            ),
            (
                NewChoice::Dependencies,
                Icon::Circle,
                tasks::dependency_title(&dependencies),
            ),
        ];
        Some(
            neo::raised(palette, radius::CARD, elevation::PANEL)
                .id("new-task-sheet")
                .absolute()
                .top(px(space::X8 + space::X18))
                .left_0()
                .right_0()
                .mx_auto()
                .w(px(desktop::NEW_TASK_SHEET_WIDTH))
                .flex()
                .flex_col()
                .gap(px(space::X14))
                .px(px(space::X18 + space::X4))
                .py(px(space::X18))
                .occlude()
                .child(sheet_header(palette, cx))
                .child(fields_section(palette, &fields))
                .child(
                    column()
                        .gap(px(space::X8))
                        .child(label(palette, WHO_WILL_RUN))
                        .child(
                            row()
                                .gap(px(space::X10))
                                .child(pill(
                                    palette,
                                    "new-task-runner",
                                    Icon::Cpu,
                                    runner,
                                    choice == Some(NewChoice::Runner),
                                    cx,
                                    |this, cx| this.pick_new_choice(NewChoice::Runner, cx),
                                ))
                                .child(spacer())
                                .child(self.default_runner_switch(default_runner, palette, cx)),
                        ),
                )
                .child(
                    row()
                        .gap(px(space::X8))
                        .children(options.map(|(which, glyph, text)| {
                            pill(
                                palette,
                                &format!("new-task-{which:?}"),
                                glyph,
                                text,
                                choice == Some(which),
                                cx,
                                move |this, cx| this.pick_new_choice(which, cx),
                            )
                        })),
                )
                .children(choices)
                .child(footer(palette, hint, valid && !submitting, valid, cx))
                .into_any_element(),
        )
    }

    /// The switch that runs the task with the workspace's default agent.
    fn default_runner_switch(
        &mut self,
        on: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        row()
            .id("new-task-default")
            .gap(px(space::X6))
            .cursor_pointer()
            .own_press()
            .chrome()
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(type_scale::CONTROL))
            .text_color(rgb(palette.secondary))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.edit_new_task(cx, |form| form.workspace_default = !form.workspace_default)
            }))
            .child(USE_DEFAULT)
            .child(switch(on, palette))
            .into_any_element()
    }

    fn pick_new_choice(&mut self, choice: NewChoice, cx: &mut Context<Self>) {
        self.edit_new_task(cx, |form| {
            form.choice = if form.choice == Some(choice) {
                None
            } else {
                Some(choice)
            };
        });
    }

    /// The choices of the option that is open.
    fn new_task_choices(
        &mut self,
        open: Option<NewChoice>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let open = open?;
        let rows: Vec<AnyElement> = match open {
            NewChoice::Priority => Self::priorities()
                .iter()
                .map(|priority| {
                    let value = priority.clone();
                    let checked = self
                        .tasks_ui
                        .form
                        .as_ref()
                        .is_some_and(|form| form.priority == value);
                    menus::on_pick(
                        menus::item(
                            palette,
                            tasks::priority_title(priority),
                            tasks::priority_title(priority),
                            checked,
                        ),
                        cx,
                        move |this, cx| {
                            let value = value.clone();
                            this.edit_new_task(cx, move |form| {
                                form.priority = value;
                                form.choice = None;
                            })
                        },
                    )
                })
                .collect(),
            NewChoice::Worktree => [
                (true, "Create a task worktree"),
                (false, "Use the workspace directly"),
            ]
            .into_iter()
            .map(|(value, text)| {
                let checked = self
                    .tasks_ui
                    .form
                    .as_ref()
                    .is_some_and(|form| form.create_worktree == value);
                menus::on_pick(
                    menus::item(palette, text, text, checked),
                    cx,
                    move |this, cx| {
                        this.edit_new_task(cx, move |form| {
                            form.create_worktree = value;
                            form.choice = None;
                        })
                    },
                )
            })
            .collect(),
            NewChoice::Dependencies => {
                let workspace = self.tasks_workspace().unwrap_or_default();
                let held: Vec<(String, String)> = self
                    .state
                    .tasks_in(&workspace)
                    .map(|task| (task.task_id.clone(), tasks::dispatch_label(task)))
                    .collect();
                held.into_iter()
                    .map(|(task_id, label)| {
                        let checked = self
                            .tasks_ui
                            .form
                            .as_ref()
                            .is_some_and(|form| form.dependencies.contains(&task_id));
                        menus::on_pick(
                            menus::item(palette, &task_id, label, checked),
                            cx,
                            move |this, cx| {
                                let task_id = task_id.clone();
                                this.edit_new_task(cx, move |form| {
                                    if !form.dependencies.remove(&task_id) {
                                        form.dependencies.insert(task_id);
                                    }
                                })
                            },
                        )
                    })
                    .collect()
            }
            NewChoice::Runner => self
                .state
                .harnesses()
                .iter()
                .filter(|harness| harness.available)
                .map(|harness| {
                    let (harness_id, name) = (harness.id.clone(), harness.name.clone());
                    let checked = self.tasks_ui.form.as_ref().is_some_and(|form| {
                        !form.workspace_default
                            && form.harness.as_deref() == Some(harness.id.as_str())
                    });
                    menus::on_pick(
                        menus::item(palette, &harness_id.clone(), name, checked),
                        cx,
                        move |this, cx| {
                            let harness_id = harness_id.clone();
                            this.edit_new_task(cx, move |form| {
                                form.workspace_default = false;
                                form.harness = Some(harness_id);
                                form.choice = None;
                            })
                        },
                    )
                })
                .collect(),
        };
        Some(menus::list(rows).max_h(px(pane_interior::STACK_SPACING * 14.0)))
    }
}

fn priority_label(priority: &TaskPriority) -> String {
    tasks::priority_title(priority).to_lowercase()
}

/// A raised pill that opens or closes the choices of an option.
fn pill(
    palette: &'static Palette,
    key: &str,
    glyph: Icon,
    text: String,
    open: bool,
    cx: &mut Context<Workbench>,
    pressed: impl Fn(&mut Workbench, &mut Context<Workbench>) + 'static,
) -> AnyElement {
    option_pill(palette, id("pill", key), glyph, text, open)
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| pressed(this, cx)))
        .into_any_element()
}

/// Create (raised) and Create and dispatch (the accent), disabled until the form is complete.
fn action(
    palette: &'static Palette,
    key: &str,
    text: &str,
    dispatch: bool,
    enabled: bool,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let base = if dispatch {
        div()
            .bg(rgb(palette.accent))
            .text_color(rgb(palette.on_accent))
    } else {
        neo::raised(palette, radius::PILL, elevation::ACTION).text_color(rgb(palette.ink))
    };
    base.id(id("action", key))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(space::X6))
        .px(px(space::X14))
        .py(px(space::X6))
        .rounded_full()
        .chrome()
        .font_weight(FontWeight::BOLD)
        .text_size(px(type_scale::CONTROL))
        .when(!enabled, |button| {
            button.opacity(pane_interior::DISABLED_OPACITY)
        })
        .when(enabled, |button| button.cursor_pointer())
        .own_press()
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if enabled {
                this.submit_new_task(dispatch, cx);
            }
        }))
        .children(dispatch.then(|| {
            icon(
                Icon::Send,
                desktop::INLINE_GLYPH,
                if dispatch {
                    palette.on_accent
                } else {
                    palette.ink
                },
            )
        }))
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

/// The title of the panel and the way to close it.
fn sheet_header(palette: &'static Palette, cx: &mut Context<Workbench>) -> AnyElement {
    row()
        .items_start()
        .child(
            column()
                .gap(px(space::X3))
                .child(chrome_text(FontWeight::BOLD, type_scale::SUBHEADING).child(TITLE))
                .child(
                    div()
                        .text_size(px(type_scale::CAPTION))
                        .text_color(rgb(palette.secondary))
                        .child(SUBTITLE),
                ),
        )
        .child(spacer())
        .child(
            row()
                .id("new-task-close")
                .cursor_pointer()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_new_task(cx)))
                .child(icon(
                    Icon::Close,
                    desktop::HEADER_ACTION_GLYPH,
                    palette.muted,
                )),
        )
        .into_any_element()
}

/// The title, the description and what must not change, each in a well.
fn fields_section(
    palette: &Palette,
    fields: &(
        Entity<ComposerField>,
        Entity<ComposerField>,
        Entity<ComposerField>,
    ),
) -> Div {
    column()
        .gap(px(space::X14))
        .child(
            column()
                .gap(px(space::X8))
                .child(label(palette, "TITLE \u{b7} REQUIRED"))
                .child(
                    neo::well(palette, radius::CONTROL, desktop::WELL_DEPTH)
                        .h(px(desktop::TASK_FIELD_HEIGHT))
                        .px(px(space::X14))
                        .flex()
                        .items_center()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(desktop::TASK_FIELD_TEXT_SIZE))
                        .child(fields.0.clone()),
                ),
        )
        .child(editor(
            palette,
            "DESCRIPTION \u{b7} REQUIRED",
            &fields.1,
            desktop::TASK_EDITOR_HEIGHT,
        ))
        .child(editor(
            palette,
            "MUST NOT CHANGE \u{b7} REQUIRED",
            &fields.2,
            desktop::TASK_EDITOR_HEIGHT,
        ))
}

/// What the form says about what is missing, and the two ways to create the task.
fn footer(
    palette: &'static Palette,
    hint: String,
    enabled: bool,
    complete: bool,
    cx: &mut Context<Workbench>,
) -> Div {
    row()
        .gap(px(space::X10))
        .pt(px(space::X6))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .chrome()
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(type_scale::CONTROL))
                .text_color(rgb(if complete {
                    palette.muted
                } else {
                    palette.warning()
                }))
                .child(SharedString::from(hint)),
        )
        .child(action(
            palette,
            "new-task-create",
            CREATE_IN_LIST,
            false,
            enabled,
            cx,
        ))
        .child(action(
            palette,
            "new-task-dispatch",
            CREATE_AND_DISPATCH,
            true,
            enabled,
            cx,
        ))
}

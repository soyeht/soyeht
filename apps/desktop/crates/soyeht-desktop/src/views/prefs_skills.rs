//! The Skills panel of Preferences, as the Mac's: the user's Soyeht skills, which every harness
//! loads. Kept simple: list, create and delete.

use super::{
    cards::{Role, button},
    preferences::{page, pill_button},
    ui::{OwnPress, Typeface, chrome_text, column, id, row},
};
use crate::{
    composer_field::{ComposerField, focus_on_press},
    neo,
    theme::{Palette, elevation, layout, pane_interior, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, Entity, FontWeight, SharedString, div, prelude::*, px,
    rgb,
};
use soyeht_protocol::Skill;

const SUMMARY: &str = "Reusable instructions every agent in Soyeht can load: natively in some harnesses, through the soyeht MCP server in the others. Ask any agent to \u{201c}create a skill that\u{2026}\u{201d}, or add one here.";

impl Workbench {
    pub(super) fn skills_page(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let listing = self.state.skills().cloned();
        let drafting = self.prefs.skill_form.is_some();
        let toolbar = row()
            .gap(px(space::X10))
            .child(
                div()
                    .chrome()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(type_scale::SMALL))
                    .text_color(rgb(palette.muted))
                    .child(SharedString::from(
                        listing
                            .as_ref()
                            .map(|list| list.user_directory.clone())
                            .unwrap_or_default(),
                    )),
            )
            .child(
                pill_button(palette, "new-skill".into(), None, "New Skill")
                    .when(drafting, |button| {
                        button.opacity(pane_interior::DISABLED_OPACITY)
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if !drafting {
                            this.begin_skill(cx);
                        }
                    })),
            );
        let form = self.skill_form(palette, cx);
        let skills: Vec<AnyElement> = listing
            .iter()
            .flat_map(|list| list.items.iter())
            .map(|skill| self.skill_row(skill, palette, cx))
            .collect();
        let empty = listing.as_ref().is_some_and(|list| list.items.is_empty());
        page(
            palette,
            "Skills",
            Some(SUMMARY),
            None,
            column()
                .gap(px(space::X16))
                .child(toolbar)
                .children(form)
                .child(
                    column()
                        .gap(px(space::X8))
                        .children(skills)
                        .children(empty.then(|| {
                            div()
                                .chrome()
                                .text_size(px(type_scale::CAPTION))
                                .text_color(rgb(palette.secondary))
                                .child("No skills yet.")
                        })),
                ),
        )
    }

    fn skill_row(
        &mut self,
        skill: &Skill,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = skill.name.clone();
        let confirming = self.prefs.skill_to_delete.as_deref() == Some(skill.name.as_str());
        let detail = match &skill.problem {
            Some(problem) => (problem.clone(), palette.danger()),
            None => (
                skill.description.clone().unwrap_or_default(),
                palette.secondary,
            ),
        };
        neo::raised(palette, radius::CONTROL, elevation::TASK_CARD)
            .id(id("skill", &skill.name))
            .flex()
            .flex_row()
            .items_start()
            .gap(px(space::X12))
            .p(px(space::X12))
            .child(
                column()
                    .flex_1()
                    .min_w_0()
                    .gap(px(space::X4))
                    .child(
                        chrome_text(FontWeight::BOLD, type_scale::BODY)
                            .child(SharedString::from(skill.name.clone())),
                    )
                    .child(
                        div()
                            .chrome()
                            .text_size(px(type_scale::CONTROL))
                            .text_color(rgb(detail.1))
                            .child(SharedString::from(detail.0)),
                    )
                    .children(skill.allowed_tools.as_ref().map(|tools| {
                        div()
                            .chrome()
                            .text_size(px(type_scale::CONTROL))
                            .text_color(rgb(palette.warning()))
                            .child(format!("Pre-approves tools in some harnesses: {tools}"))
                    })),
            )
            .child(
                pill_button(
                    palette,
                    id("delete-skill", &skill.name),
                    None,
                    if confirming {
                        "Delete the skill and its folder?"
                    } else {
                        "Delete"
                    },
                )
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.delete_skill(&name, cx)),
                ),
            )
            .into_any_element()
    }

    /// The form that makes a skill: a name, when to use it, and what it tells the agent.
    fn skill_form(&mut self, palette: &'static Palette, cx: &mut Context<Self>) -> Option<Div> {
        let form = self.prefs.skill_form.as_ref()?;
        let fields = [
            (&form.name, "name (lowercase-with-hyphens)"),
            (
                &form.description,
                "When to use it: what it does and when an agent should load it",
            ),
            (&form.instructions, "Instructions"),
        ];
        let ready =
            !form.name.read(cx).draft().is_blank() && !form.description.read(cx).draft().is_blank();
        let wells: Vec<AnyElement> = fields
            .into_iter()
            .map(|(field, hint)| well_field(field.clone(), hint, palette, cx))
            .collect();
        Some(
            column().gap(px(space::X8)).children(wells).child(
                row()
                    .justify_end()
                    .gap(px(space::X8))
                    .child(
                        button("cancel-skill".into(), Role::Secondary, "Cancel", palette)
                            .own_press()
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_skill(cx)),
                            ),
                    )
                    .child(
                        button("create-skill".into(), Role::Primary, "Create", palette)
                            .when(!ready, |button| {
                                button.opacity(pane_interior::DISABLED_OPACITY)
                            })
                            .own_press()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if ready {
                                    this.create_skill(cx);
                                }
                            })),
                    ),
            ),
        )
    }
}

/// A well that holds one text field, with `hint` while it is empty.
fn well_field(
    field: Entity<ComposerField>,
    hint: &str,
    palette: &'static Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    field.update(cx, |field, _| field.style(palette, hint));
    focus_on_press(
        div()
            .w_full()
            .min_h(px(layout::SEARCH_FIELD_HEIGHT))
            .flex()
            .items_center()
            .px(px(space::X10))
            .py(px(space::X6))
            .rounded(px(radius::COMPACT))
            .bg(rgb(palette.well))
            .text_size(px(type_scale::CONTROL)),
        &field,
    )
    .child(div().flex_1().min_w_0().child(field))
    .into_any_element()
}

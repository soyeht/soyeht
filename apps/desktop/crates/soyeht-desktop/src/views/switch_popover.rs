//! The popover of the harness chip, as the Mac's `AgentHarnessSwitchPicker` draws it: what
//! switching does, the harnesses to pick from, the model and effort of the one picked, the
//! button that confirms, and the account of the agent that runs now.

use super::{
    cards::{Role, button},
    harness_menu::{Pick, Picker},
    menus,
    ui::{OwnPress, column, hint, icon, id, row, rule},
};
use crate::workbench::Workbench;
use crate::{
    assets::Icon,
    theme::{Palette, desktop, pane_interior, radius, space, type_scale},
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, SharedString, div, prelude::*, px, rgb,
};
use soyeht_client::{
    account::{self, AccountActions},
    catalogue::{self, ModelState},
    harness_picker, switch,
};
use soyeht_protocol::{ConfigOption, HarnessInfo, Pane};
use std::rc::Rc;

impl Workbench {
    /// The body of the popover of `pane`'s harness chip.
    pub(super) fn switch_popover(
        &mut self,
        pane: &Pane,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let pane_id = pane.pane_id.clone();
        let Some(draft) = self.switch_draft(&pane_id) else {
            return column();
        };
        let search = draft.search.clone();
        let (chosen, model, effort) = (
            draft.harness.clone(),
            draft.model.clone(),
            draft.effort.clone(),
        );
        let models_open = draft.models_open;
        search.update(cx, |field, _| field.style(palette, "Search agents"));
        let query = search.read(cx).draft().text().to_owned();
        let running = pane.agent.as_ref().map(|agent| agent.harness.clone());

        let pick: Pick = Rc::new(|this, harness, cx| this.choose_switch_harness(harness, cx));
        let rows = self.harness_rows(
            Picker {
                excluding: running.as_deref(),
                query: &query,
                chosen: chosen.as_deref(),
                pick_unavailable: true,
            },
            palette,
            &pick,
            cx,
        );
        let target = self.switch_target(&pane_id).cloned();
        let configuration = self.configuration(
            target.as_ref(),
            model.as_deref(),
            effort.as_deref(),
            models_open,
            palette,
            cx,
        );
        let connected = self.state.connection == soyeht_client::Connection::Connected;
        let allowed = switch::can_switch(
            connected,
            pane.agent.as_ref(),
            self.state
                .agent(&pane_id)
                .and_then(|view| view.harness_switch.as_ref()),
        );
        let ready = allowed && target.as_ref().is_some_and(|target| target.available);
        let account = AccountActions::new(pane.agent.as_ref(), Some(&pane.state), connected)
            .map(|actions| account_section(&pane_id, &actions, palette, cx));

        column()
            .w(px(desktop::SWITCH_POPOVER_CONTENT_WIDTH))
            .gap(px(space::X12))
            .child(header(palette))
            .child(
                row()
                    .gap(px(space::X8))
                    .child(search_field(search, palette))
                    .child(
                        row()
                            .id("rescan-harnesses")
                            .size(px(desktop::SEARCH_FIELD_HEIGHT))
                            .flex_none()
                            .justify_center()
                            .rounded(px(radius::COMPACT))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(palette.hover)))
                            .tooltip(hint("Rescan installed agents", palette))
                            .own_press()
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.rescan_harnesses(cx)
                                }),
                            )
                            .child(icon(
                                Icon::Refresh,
                                desktop::COMPOSER_GLYPH,
                                palette.secondary,
                            )),
                    ),
            )
            .child(
                div()
                    .id("switch-harnesses")
                    .max_h(px(desktop::HARNESS_LIST_HEIGHT))
                    .flex_none()
                    .overflow_y_scroll()
                    .child(menus::list(rows)),
            )
            .child(rule(palette.well, desktop::HAIRLINE))
            .child(configuration)
            .child(confirm_button(
                &pane_id,
                target.as_ref(),
                ready,
                palette,
                cx,
            ))
            .children(account)
    }

    /// The model and effort of the harness picked, or why it cannot be switched to.
    fn configuration(
        &mut self,
        target: Option<&HarnessInfo>,
        model: Option<&str>,
        effort: Option<&str>,
        models_open: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(target) = target else {
            return column().child(caption(palette, "Choose an agent to continue."));
        };
        let reason = harness_picker::status_reason(target);
        if !target.available {
            return column().child(
                row()
                    .gap(px(space::X6))
                    .text_size(px(type_scale::CAPTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette.muted))
                    .child(icon(Icon::Alert, desktop::INLINE_GLYPH, palette.muted))
                    .child(div().flex_1().min_w_0().child(reason)),
            );
        }
        let catalogue = self.state.catalogue(&target.id).cloned();
        let failure = self.state.catalogue_failure(&target.id).map(str::to_owned);
        let state = catalogue::model_state(
            catalogue.as_ref(),
            catalogue.is_none() && failure.is_none(),
            failure.is_some(),
        );
        let mut section =
            column()
                .gap(px(space::X8))
                .children(harness_picker::picker_notice(target).map(|notice| {
                    div()
                        .text_size(px(type_scale::SMALL))
                        .text_color(rgb(palette.secondary))
                        .child(notice)
                }));
        section = section.child(menus::heading(palette, "Model"));
        section = section.child(model_field(
            catalogue.as_ref(),
            model,
            state,
            models_open,
            palette,
            cx,
        ));
        if models_open && let Some(catalogue) = &catalogue {
            section = section.child(model_choices(catalogue, model, palette, cx));
        }
        section = section.children(model_notes(
            catalogue.as_ref(),
            model,
            state,
            failure.is_some(),
            &target.id,
            palette,
            cx,
        ));
        if let Some(option) = catalogue
            .as_ref()
            .and_then(|catalogue| catalogue.effort_option())
            && !option.options.is_empty()
        {
            section = section
                .child(menus::heading(palette, "Effort"))
                .child(effort_segments(option, effort, palette, cx));
        }
        section
    }
}

fn header(palette: &Palette) -> Div {
    column()
        .gap(px(space::X4))
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .text_size(px(type_scale::BODY))
                .child(switch::TITLE),
        )
        .child(caption(palette, switch::HANDOVER_EXPLANATION))
}

/// A line of muted explanation.
fn caption(palette: &Palette, text: &str) -> Div {
    div()
        .text_size(px(type_scale::CAPTION))
        .text_color(rgb(palette.muted))
        .child(SharedString::from(text.to_owned()))
}

/// The well that holds the search line.
fn search_field(
    search: gpui::Entity<crate::composer_field::ComposerField>,
    palette: &Palette,
) -> Div {
    well_row(palette)
        .flex_1()
        .min_w_0()
        .h(px(desktop::SEARCH_FIELD_HEIGHT))
        .child(div().flex_1().min_w_0().overflow_hidden().child(search))
}

/// A line set into the well, as the Mac's search and model fields are.
pub(super) fn well_row(palette: &Palette) -> Div {
    row()
        .px(px(space::X10))
        .rounded(px(radius::COMPACT))
        .bg(rgb(palette.well))
        .text_size(px(type_scale::CAPTION))
        .font_weight(FontWeight::SEMIBOLD)
}

/// A line of small warning text with its mark.
fn warning_row(palette: &Palette) -> Div {
    row()
        .gap(px(space::X6))
        .text_size(px(type_scale::SMALL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(palette.warning()))
        .child(icon(Icon::Alert, desktop::INLINE_GLYPH, palette.warning()))
}

/// The field that shows the model chosen and unfolds the list of models.
fn model_field(
    catalogue: Option<&catalogue::Catalogue>,
    model: Option<&str>,
    state: ModelState,
    open: bool,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let label = if state == ModelState::Loading {
        "Loading models\u{2026}".to_owned()
    } else {
        model
            .and_then(|id| {
                catalogue.and_then(|catalogue| catalogue.models.iter().find(|known| known.id == id))
            })
            .map_or_else(|| "Agent default".to_owned(), harness_picker::model_label)
    };
    let enabled = state != ModelState::Loading;
    well_row(palette)
        .id("switch-model")
        .h(px(desktop::MODEL_FIELD_HEIGHT))
        .gap(px(space::X8))
        .text_color(rgb(if enabled { palette.ink } else { palette.muted }))
        .when(enabled, |field| field.cursor_pointer())
        .own_press()
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if enabled {
                this.toggle_switch_models(cx);
            }
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(SharedString::from(label)),
        )
        .child(super::ui::disclosure_chevron(open, palette.muted))
        .into_any_element()
}

/// The models of the harness, unfolded under the field: "Agent default" first.
fn model_choices(
    catalogue: &catalogue::Catalogue,
    chosen: Option<&str>,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> Div {
    let default = menus::on_pick(
        menus::item(palette, "agent-default", "Agent default", chosen.is_none()),
        cx,
        |this, cx| this.choose_switch_model(None, cx),
    );
    let models = catalogue.models.iter().map(|model| {
        let picked = model.id.clone();
        menus::on_pick(
            menus::item(
                palette,
                &model.id,
                harness_picker::model_label(model),
                chosen == Some(model.id.as_str()),
            ),
            cx,
            move |this, cx| this.choose_switch_model(Some(picked.clone()), cx),
        )
    });
    menus::list(std::iter::once(default).chain(models).collect())
}

/// What the model list says besides its choices: that loading failed (with a retry), that the
/// model chosen was refused before, and that there are no choices.
#[allow(clippy::too_many_arguments)]
fn model_notes(
    catalogue: Option<&catalogue::Catalogue>,
    model: Option<&str>,
    state: ModelState,
    failed: bool,
    harness: &str,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> Vec<AnyElement> {
    let mut notes = Vec::new();
    if failed {
        let harness = harness.to_owned();
        notes.push(
            warning_row(palette)
                .child(div().flex_1().child("The models could not be loaded."))
                .child(
                    button(
                        id("retry-models", &harness),
                        Role::Secondary,
                        "Retry",
                        palette,
                    )
                    .own_press()
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| this.retry_catalogue(&harness, cx),
                    )),
                )
                .into_any_element(),
        );
    }
    let refusal = model
        .and_then(|id| {
            catalogue.and_then(|catalogue| catalogue.models.iter().find(|known| known.id == id))
        })
        .and_then(|model| model.observed_refusal.as_ref());
    if let Some(refusal) = refusal {
        notes.push(
            warning_row(palette)
                .child(div().flex_1().min_w_0().child(format!(
                    "This agent previously refused this model: {}",
                    refusal.reason
                )))
                .into_any_element(),
        );
    }
    let empty = match state {
        ModelState::Empty => Some("This agent provided no model choices."),
        ModelState::Unavailable => Some("Models will appear after this agent starts."),
        _ => None,
    };
    notes.extend(empty.map(|text| {
        div()
            .text_size(px(type_scale::SMALL))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(palette.muted))
            .child(text)
            .into_any_element()
    }));
    notes
}

/// The efforts of the harness as a capsule of segments, the one chosen raised on the face.
fn effort_segments(
    option: &ConfigOption,
    chosen: Option<&str>,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> Div {
    let segments = option.options.iter().map(|choice| {
        let picked = choice.value.clone();
        let selected = chosen == Some(choice.value.as_str());
        div()
            .id(id("effort", &choice.value))
            .flex_1()
            .py(px(space::X6))
            .flex()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(type_scale::CONTROL))
            .font_weight(if selected {
                FontWeight::BOLD
            } else {
                FontWeight::SEMIBOLD
            })
            .text_color(rgb(palette.ink))
            .when(selected, |segment| segment.bg(rgb(palette.face)))
            .own_press()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.choose_switch_effort(picked.clone(), cx)
            }))
            .child(SharedString::from(choice.name.clone()))
    });
    row()
        .p(px(desktop::EFFORT_SEGMENT_INSET))
        .gap(px(space::X2))
        .rounded_full()
        .bg(rgb(palette.well))
        .children(segments)
}

/// "Restart conversation with ...": dimmed until a harness that can start is picked.
fn confirm_button(
    pane_id: &str,
    target: Option<&HarnessInfo>,
    enabled: bool,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let pane = pane_id.to_owned();
    action_pill(
        id("switch-confirm", pane_id),
        Role::Primary,
        enabled,
        harness_picker::switch_label(target),
        palette,
        cx,
        move |this, cx| this.confirm_switch(&pane, cx),
    )
}

/// A full-width pill of `role` that runs `press` unless it is dimmed.
fn action_pill(
    key: SharedString,
    role: Role,
    enabled: bool,
    label: String,
    palette: &Palette,
    cx: &mut Context<Workbench>,
    press: impl Fn(&mut Workbench, &mut Context<Workbench>) + 'static,
) -> AnyElement {
    button(key, role, label, palette)
        .flex()
        .items_center()
        .justify_center()
        .w_full()
        .py(px(space::X8))
        .text_size(px(type_scale::CONTROL))
        .when(!enabled, |button| {
            button
                .opacity(pane_interior::DISABLED_OPACITY)
                .cursor_default()
        })
        .own_press()
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if enabled {
                press(this, cx);
            }
        }))
        .into_any_element()
}

/// The account of the agent that runs now: sign in again as someone else, or sign out.
fn account_section(
    pane_id: &str,
    actions: &AccountActions,
    palette: &Palette,
    cx: &mut Context<Workbench>,
) -> Div {
    let buttons: Vec<AnyElement> = actions
        .items()
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let role = if item.is_destructive() {
                Role::Destructive
            } else if index == 0 {
                Role::Primary
            } else {
                Role::Secondary
            };
            let (pane, action, enabled) =
                (pane_id.to_owned(), item.action.clone(), actions.enabled);
            action_pill(
                id("account", &format!("{pane_id}-{index}")),
                role,
                enabled,
                item.title,
                palette,
                cx,
                move |this, cx| this.run_account_action(&pane, &action, cx),
            )
        })
        .collect();
    column()
        .gap(px(space::X8))
        .child(rule(palette.well, desktop::HAIRLINE))
        .child(
            div()
                .font_weight(FontWeight::EXTRA_BOLD)
                .text_size(px(pane_interior::HEADING_POINT_SIZE))
                .child(account::SECTION_TITLE),
        )
        .child(
            div()
                .text_size(px(pane_interior::CAPTION_POINT_SIZE))
                .text_color(rgb(palette.secondary))
                .child(account::EXPLANATION),
        )
        .children(buttons)
}

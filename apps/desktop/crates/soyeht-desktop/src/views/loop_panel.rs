//! The loop panel that the pane header's loop button opens, as the Mac's `PaneAutomationPanel`
//! and the design's "Loop da pane" panel draw it: the role and the on/off switch, the prompt in a
//! well, when it runs, the three options, what the loop promises, and a status line with Cancel
//! and Apply. It is chrome (a popover), so its controls are raised and its prompt a well.

use super::{
    menus,
    prelude::*,
    ui::{field_well, mono_note, option_pill, switch},
};
use crate::{
    loop_form::Choice,
    neo,
    theme::{Palette, desktop, elevation, pane_interior, radius, space, type_scale},
};
use soyeht_client::loops::{self, Form};
use soyeht_protocol::{AgentPaneRole, LoopStopWhen, Pane};

impl Workbench {
    pub(super) fn loop_panel(
        &mut self,
        pane: &Pane,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        self.ensure_loop_form(pane, cx);
        let rounds = self
            .state
            .loop_of(&pane.pane_id)
            .and_then(|held| held.rounds)
            .unwrap_or(0);
        let Some(held) = self.loop_form(&pane.pane_id) else {
            return div();
        };
        let (form, role, choice) = (held.form.clone(), held.role.clone(), held.choice);
        let prompt = held.prompt.clone();
        let can_apply = loops::can_apply(form.enabled, prompt.read(cx).draft().text());
        let pane_id = pane.pane_id.clone();
        let applied = pane.clone();
        column()
            .w(px(desktop::LOOP_PANEL_CONTENT_WIDTH))
            .gap(px(space::X12))
            .child(self.loop_header(&pane_id, &form, &role, palette, cx))
            .child(heading(palette, loops::PROMPT_HEADING))
            .child(
                field_well(palette, &prompt, desktop::LOOP_PROMPT_HEIGHT)
                    .p(px(space::X10))
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(px(type_scale::CAPTION))
                    .child(prompt),
            )
            .child(heading(palette, loops::WHEN_HEADING))
            .child(idle_trigger(palette))
            .child(self.loop_options(&pane_id, &form, choice, palette, cx))
            .children(self.loop_choices(&pane_id, &form, &role, choice, palette, cx))
            .child(rules(palette))
            .child(
                row()
                    .gap(px(space::X8))
                    .pt(px(space::X6))
                    .chrome()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(type_scale::CONTROL))
                    .text_color(rgb(palette.accent))
                    .child(
                        mono_note(palette)
                            .flex_1()
                            .min_w_0()
                            .child(SharedString::from(loops::status(
                                form.enabled,
                                &pane.name,
                                rounds,
                            ))),
                    )
                    .child(
                        div()
                            .id(id("loop-cancel", &pane_id))
                            .cursor_pointer()
                            .own_press()
                            .on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| this.close_menu(cx)),
                            )
                            .child(loops::CANCEL),
                    )
                    .child(
                        div()
                            .id(id("loop-apply", &pane_id))
                            .when(!can_apply, |apply| {
                                apply.opacity(pane_interior::DISABLED_OPACITY)
                            })
                            .when(can_apply, |apply| apply.cursor_pointer())
                            .own_press()
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.apply_loop(&applied, cx)
                            }))
                            .child(loops::APPLY),
                    ),
            )
    }

    /// The role menu on the title, and the switch that turns the loop on.
    fn loop_header(
        &mut self,
        pane_id: &str,
        form: &Form,
        role: &AgentPaneRole,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (roles, toggled) = (pane_id.to_owned(), pane_id.to_owned());
        let lead = *role == AgentPaneRole::Chief;
        row()
            .gap(px(space::X8))
            .child(
                row()
                    .id(id("loop-role", pane_id))
                    .gap(px(space::X8))
                    .cursor_pointer()
                    .own_press()
                    .tooltip(super::ui::hint(
                        format!("Pane role: {}", if lead { "lead" } else { "worker" }),
                        palette,
                    ))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.show_choices(&roles, Choice::Role, cx)
                    }))
                    .child(icon(Icon::Timer, desktop::COMPOSER_GLYPH, palette.accent))
                    .child(
                        chrome_text(FontWeight::BOLD, desktop::LOOP_PANEL_TITLE_SIZE)
                            .text_color(rgb(palette.ink))
                            .child(loops::TITLE),
                    ),
            )
            .child(spacer())
            .child(
                row()
                    .id(id("loop-toggle", pane_id))
                    .cursor_pointer()
                    .own_press()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_loop(&toggled, cx)
                    }))
                    .child(switch(form.enabled, palette)),
            )
    }

    /// The interval, the number of rounds and when to stop, each a raised pill.
    fn loop_options(
        &mut self,
        pane_id: &str,
        form: &Form,
        open: Option<Choice>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let options = [
            (
                Choice::Interval,
                Icon::Timer,
                loops::interval_label(form.interval),
            ),
            (
                Choice::Rounds,
                Icon::Hash,
                loops::round_label(form.max_rounds),
            ),
            (
                Choice::Stop,
                Icon::Calendar,
                loops::stop_label(&form.stop_when).to_owned(),
            ),
        ];
        row()
            .gap(px(space::X8))
            .children(options.map(|(choice, glyph, label)| {
                let owner = pane_id.to_owned();
                option_pill(
                    palette,
                    id("loop-option", &format!("{pane_id}-{choice:?}")),
                    glyph,
                    label,
                    open == Some(choice),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.show_choices(&owner, choice, cx)
                }))
                .into_any_element()
            }))
    }

    /// The choices of the option that is open, one row each, the current one checked.
    fn loop_choices(
        &mut self,
        pane_id: &str,
        form: &Form,
        role: &AgentPaneRole,
        open: Option<Choice>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let open = open?;
        let owner = pane_id.to_owned();
        let rows: Vec<AnyElement> = match open {
            Choice::Role => [
                (loops::WORKER_ROLE, AgentPaneRole::Worker),
                (loops::LEAD_ROLE, AgentPaneRole::Chief),
            ]
            .into_iter()
            .map(|(label, value)| {
                let (owner, checked) = (owner.clone(), *role == value);
                menus::on_pick(
                    menus::item(palette, label, label, checked),
                    cx,
                    move |this, cx| this.pick_role(&owner, value.clone(), cx),
                )
            })
            .collect(),
            Choice::Interval => loops::INTERVAL_CHOICES
                .into_iter()
                .map(|seconds| {
                    let owner = owner.clone();
                    let label = loops::interval_label(seconds);
                    menus::on_pick(
                        menus::item(palette, &label, label.clone(), seconds == form.interval),
                        cx,
                        move |this, cx| this.pick_interval(&owner, seconds, cx),
                    )
                })
                .collect(),
            Choice::Rounds => loops::ROUND_CHOICES
                .into_iter()
                .map(|rounds| {
                    let owner = owner.clone();
                    let label = loops::round_label(rounds);
                    menus::on_pick(
                        menus::item(palette, &label, label.clone(), rounds == form.max_rounds),
                        cx,
                        move |this, cx| this.pick_rounds(&owner, rounds, cx),
                    )
                })
                .collect(),
            Choice::Stop => loops::STOP_CHOICES
                .into_iter()
                .map(|stop_when: LoopStopWhen| {
                    let owner = owner.clone();
                    let label = loops::stop_choice_title(&stop_when);
                    menus::on_pick(
                        menus::item(palette, label, label, stop_when == form.stop_when),
                        cx,
                        move |this, cx| this.pick_stop(&owner, stop_when.clone(), cx),
                    )
                })
                .collect(),
        };
        Some(menus::list(rows))
    }
}

/// A small heading over a part of the panel.
fn heading(palette: &Palette, text: &str) -> Div {
    menus::heading(palette, text)
}

/// "Every interval" and "When idle" in a capsule: the loop only runs when the pane is idle, so
/// the second is the one raised.
fn idle_trigger(palette: &'static Palette) -> Div {
    let segment = |raised: bool| {
        let frame = if raised {
            neo::raised(palette, radius::PILL, elevation::TOGGLE)
        } else {
            div()
        };
        frame
            .flex_1()
            .py(px(space::X6))
            .flex()
            .justify_center()
            .chrome()
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(type_scale::CONTROL))
            .text_color(rgb(if raised { palette.ink } else { palette.muted }))
    };
    row()
        .p(px(space::X3))
        .rounded_full()
        .bg(rgb(palette.well))
        .child(segment(false).child(loops::EVERY_INTERVAL))
        .child(segment(true).child(loops::WHEN_IDLE))
}

/// What the loop promises, each with a check.
fn rules(palette: &Palette) -> Div {
    column()
        .gap(px(space::X5))
        .px(px(space::X2))
        .pt(px(space::X4))
        .children(loops::RULES.map(|rule| {
            row()
                .items_start()
                .gap(px(space::X6))
                .chrome()
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(type_scale::CONTROL))
                .text_color(rgb(palette.secondary))
                .child(icon(Icon::Check, desktop::INLINE_GLYPH, palette.success()))
                .child(rule)
        }))
}

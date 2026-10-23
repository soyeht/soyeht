//! The composer of the approved frames: one flat card with the message line on top and one row
//! of controls under it: "+", the harness, model, effort and permission chips, the context ring,
//! the mic and a round send button (stop while the agent runs).

use super::{
    agent::notice_line,
    attachments::chip_view,
    menus,
    ui::{OwnPress, Typeface, column, hint, icon, id, logo, row, sideways_scroller},
    usage_ring::ring_canvas,
};
use crate::{
    assets::{self, Icon},
    open_menu::Menu,
    theme::{Palette, composer, desktop, layout, pane_interior, pane_layout},
    workbench::Workbench,
};
use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    MouseButton, ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    div, prelude::FluentBuilder, px, rgb,
};
use soyeht_client::{
    agent::{Action, Controls, Menu as ConfigMenu},
    attachments::Chip,
    harness_picker,
    switch::{self, Phase},
};
use soyeht_protocol::{Pane, client_policy};

impl Workbench {
    pub(super) fn composer(
        &mut self,
        pane: &Pane,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane.pane_id.clone();
        let field = self.composer_field(&pane_id, cx);
        let context = self
            .state
            .agent(&pane_id)
            .and_then(|view| view.context_fraction());
        let connected = self.state.connection == soyeht_client::Connection::Connected;
        let harness_name = pane
            .agent
            .as_ref()
            .and_then(|agent| self.state.harness_name(&agent.harness))
            .map(str::to_owned);
        let has_draft = !field.read(cx).draft().is_blank();
        let controls = Controls::new(pane, context, harness_name.as_deref(), has_draft, connected);
        let placeholder = controls.as_ref().map_or_else(
            || "Starting the agent\u{2026}".to_owned(),
            |c| c.placeholder.clone(),
        );
        field.update(cx, |field, _| field.style(palette, &placeholder));
        let focus_id = pane_id.clone();
        let rows = self.slash_rows(&pane_id, cx);
        field.update(cx, |field, _| field.complete_with_list(!rows.is_empty()));
        let slash_list = self.slash_list(&rows, palette, &pane_id, cx);
        let attachments = self.attachment_row(&pane_id, palette, cx);

        let controls_row = match &controls {
            Some(controls) => self.control_row(pane, controls, palette, cx),
            None => row().into_any_element(),
        };
        card_frame(palette)
            .relative()
            .mx(px(pane_interior::CARD_PADDING))
            .mb(px(pane_interior::CARD_VERTICAL_PADDING))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.focus_input(&focus_id, window, cx)),
            )
            .children(attachments)
            .child(field)
            .child(controls_row)
            .children(slash_list)
            .into_any_element()
    }

    /// The attachment chips above the message line, and the latest refusal; nothing while there
    /// is neither.
    fn attachment_row(
        &mut self,
        pane_id: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let staged = self.staged(pane_id).filter(|staged| staged.is_shown())?;
        let chips: Vec<Chip> = staged.items().iter().map(Chip::of_draft).collect();
        let notice = staged.notice().map(str::to_owned);
        let chips = chips.iter().map(|chip| {
            let (pane_id, chip_id) = (pane_id.to_owned(), chip.id.clone());
            let remove = row()
                .id(id("attachment-remove", &chip.id))
                .flex_none()
                .cursor_pointer()
                .own_press()
                .tooltip(hint(format!("Remove {}", chip.name), palette))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.remove_attachment(&pane_id, &chip_id, cx)
                }))
                .child(icon(Icon::Close, desktop::INLINE_GLYPH, palette.muted));
            chip_view(chip, palette).child(remove).into_any_element()
        });
        let strip = row()
            .flex_none()
            .gap(px(pane_interior::COMPACT_SPACING))
            .children(chips);
        let scroll = self.attachment_scroll(pane_id);
        Some(
            column()
                .gap(px(pane_interior::COMPACT_SPACING))
                .child(
                    sideways_scroller(id("composer-attachments", pane_id), &scroll, strip, palette)
                        .flex_none(),
                )
                .children(notice.map(|text| notice_line(&text, palette)))
                .into_any_element(),
        )
    }

    fn control_row(
        &mut self,
        pane: &Pane,
        controls: &Controls,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane.pane_id.clone();
        let connected = self.state.connection == soyeht_client::Connection::Connected;
        let enabled = connected && !controls.busy;

        let mut chips: Vec<AnyElement> = Vec::new();
        chips.push(self.harness_chip(pane, connected, palette, cx));
        let model_or_effort = controls.model.is_some() || controls.effort.is_some();
        if model_or_effort {
            chips.push(separator(palette));
        }
        for menu in [&controls.model, &controls.effort].into_iter().flatten() {
            chips.push(self.config_chip(&pane_id, menu, None, enabled, palette, cx));
        }
        if let Some(menu) = &controls.permission {
            chips.push(separator(palette));
            chips.push(self.config_chip(&pane_id, menu, Some(Icon::Shield), enabled, palette, cx));
        }

        let notice_for = |button: &'static str| Menu::Notice {
            pane_id: pane_id.clone(),
            button,
        };
        row()
            .gap(px(composer::CONTROL_SPACING))
            .child(self.attach_button(&pane_id, palette, cx))
            // As on the Mac, the chips keep their size and the strip scrolls sideways when the
            // pane is too narrow for all of them. A fade at an end that hides more says so, and
            // the wheel moves the strip only sideways.
            .child(chip_strip(
                &self.chip_scroll(&pane_id),
                &pane_id,
                chips,
                palette,
            ))
            .child(context_gauge(controls, palette))
            .child(self.round_button(
                "mic",
                Icon::Mic,
                "Dictation is not available in the desktop client yet.",
                notice_for("mic"),
                palette,
                cx,
            ))
            .child(self.send_button(&pane_id, controls.action, palette, cx))
            .into_any_element()
    }

    /// The "+" of the control row: the file picker.
    fn attach_button(
        &mut self,
        pane_id: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane_id.to_owned();
        div()
            .relative()
            .flex_none()
            .child(
                round_frame("attach", Icon::Plus, palette)
                    .tooltip(hint("Attach files", palette))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.pick_attachments(&pane_id, cx)
                    })),
            )
            .into_any_element()
    }

    /// A round 28-point icon button of the control row. The desktop client does not serve what
    /// it stands for yet, so a click says so beside the button.
    fn round_button(
        &mut self,
        name: &'static str,
        glyph: Icon,
        message: &'static str,
        menu: Menu,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.menus.is_open(&menu);
        let toggled = menu.clone();
        div()
            .relative()
            .flex_none()
            .child(round_frame(name, glyph, palette).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| this.toggle_menu(toggled.clone(), cx),
            )))
            .when(open, |slot| {
                slot.child(menus::popup(
                    palette,
                    Anchor::BottomLeft,
                    (0.0, -desktop::POPUP_GAP),
                    menus::note(palette, message),
                    cx,
                ))
            })
            .into_any_element()
    }

    fn send_button(
        &mut self,
        pane_id: &str,
        action: Action,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane_id.to_owned();
        let (stopping, enabled) = match action {
            Action::Send { enabled } => (false, enabled),
            Action::Stop { enabled } => (true, enabled),
        };
        ink_button(id("composer-send", &pane_id), enabled, palette)
            .own_press()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if enabled {
                    if stopping {
                        this.cancel_turn(&pane_id);
                    } else {
                        this.submit_prompt(&pane_id, cx);
                    }
                }
            }))
            .child(if stopping {
                div()
                    .size(px(composer::STOP_SIZE))
                    .rounded(px(composer::STOP_RADIUS))
                    .bg(rgb(palette.face))
                    .into_any_element()
            } else {
                icon(Icon::ArrowUp, desktop::COMPOSER_GLYPH, palette.face).into_any_element()
            })
            .into_any_element()
    }

    /// The harness chip: the harness the pane runs, or the one it is switching to while a switch
    /// runs. It opens the switch popover.
    fn harness_chip(
        &mut self,
        pane: &Pane,
        connected: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane.pane_id.as_str();
        let current = pane
            .agent
            .as_ref()
            .map_or("", |agent| agent.harness.as_str());
        let view = self.state.agent(pane_id);
        let switching = view
            .and_then(|view| view.harness_switch.as_ref())
            .filter(|switch| switch.phase == Phase::Switching)
            .map(|switch| switch.to.clone());
        let (label, _) = harness_picker::chip_title(
            self.state.harnesses(),
            current,
            switching.as_deref(),
            Some(&pane.name),
        );
        let enabled = switch::can_switch(
            connected,
            pane.agent.as_ref(),
            view.and_then(|view| view.harness_switch.as_ref()),
        );
        let menu = Menu::Harness(pane_id.to_owned());
        let open = self.menus.is_open(&menu);
        let mark = assets::logo_path(switching.as_deref().unwrap_or(current))
            .map(|path| logo(path, composer::HARNESS_LOGO_SIZE, palette.ink).into_any_element());
        let popover = open.then(|| {
            (
                self.switch_popover(pane, palette, cx),
                desktop::SWITCH_POPOVER_MAXIMUM_HEIGHT
                    .min(self.window.height - 2.0 * desktop::PREFERENCES_INSET),
            )
        });
        ControlChip {
            key: id("harness-chip", pane_id),
            label: &label,
            emphasized: true,
            leading: mark,
            enabled,
            menu,
            popup: popover,
        }
        .build(palette, cx)
    }

    fn config_chip(
        &mut self,
        pane_id: &str,
        menu: &ConfigMenu,
        leading: Option<Icon>,
        enabled: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open_menu = Menu::Config {
            pane_id: pane_id.to_owned(),
            config_id: menu.config_id.clone(),
        };
        let open = self.menus.is_open(&open_menu);
        let rows: Vec<AnyElement> = menu
            .choices
            .iter()
            .map(|(value, name)| {
                let (pane_id, config_id, picked) =
                    (pane_id.to_owned(), menu.config_id.clone(), value.clone());
                let checked = menu.current.as_ref() == Some(value);
                menus::on_pick(
                    menus::item(palette, value, name.clone(), checked),
                    cx,
                    move |this, cx| {
                        if checked {
                            this.close_menu(cx);
                        } else {
                            this.set_config(&pane_id, &config_id, &picked, cx);
                        }
                    },
                )
            })
            .collect();
        ControlChip {
            key: id("config-chip", &format!("{pane_id}-{}", menu.config_id)),
            label: menu.label(),
            emphasized: false,
            leading: leading.map(|glyph| {
                icon(glyph, composer::SHIELD_SIZE, palette.secondary).into_any_element()
            }),
            enabled,
            menu: open_menu,
            popup: open.then(|| {
                (
                    menus::list(rows),
                    layout::WORKSPACE_OVERFLOW_POPOVER_MAXIMUM_HEIGHT,
                )
            }),
        }
        .build(palette, cx)
    }
}

/// The frame of a round 28-point icon button of the control row: a click does what its caller
/// says.
fn round_frame(name: &str, glyph: Icon, palette: &Palette) -> gpui::Stateful<Div> {
    row()
        .id(id("composer", name))
        .size(px(composer::CONTROL_HEIGHT))
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(palette.hover)))
        .own_press()
        .child(icon(glyph, desktop::COMPOSER_GLYPH, palette.secondary))
}

/// The flat card of the composer: the message line over the row of controls.
pub(super) fn card_frame(palette: &Palette) -> Div {
    column()
        .flex_none()
        .px(px(pane_interior::CARD_PADDING))
        .py(px(pane_interior::CARD_VERTICAL_PADDING))
        .gap(px(composer::ROW_SPACING))
        .rounded(px(pane_interior::COMPOSER_RADIUS))
        .bg(rgb(palette.canvas))
        .text_size(px(pane_interior::BODY_POINT_SIZE))
}

/// The round ink button at the end of the control row, which sends. It shows a pointer and
/// presses while `enabled`; what a click does is the caller's.
pub(super) fn ink_button(
    key: impl Into<gpui::ElementId>,
    enabled: bool,
    palette: &Palette,
) -> gpui::Stateful<Div> {
    row()
        .id(key)
        .w(px(composer::SEND_WIDTH))
        .h(px(composer::CONTROL_HEIGHT))
        .flex_none()
        .justify_center()
        .rounded_full()
        .bg(rgb(palette.ink))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .active(|style| style.opacity(pane_interior::PRESSED_OPACITY))
        })
}

/// A borderless chip of the control row: an optional mark, the label and a chevron.
pub(super) struct ControlChip<'a> {
    pub key: SharedString,
    pub label: &'a str,
    pub emphasized: bool,
    pub leading: Option<AnyElement>,
    pub enabled: bool,
    /// The menu a click opens.
    pub menu: Menu,
    /// What the open menu shows, and the most it may be tall.
    pub popup: Option<(Div, f32)>,
}

impl ControlChip<'_> {
    pub(super) fn build(
        self,
        palette: &'static Palette,
        cx: &mut Context<Workbench>,
    ) -> AnyElement {
        let Self {
            key,
            label,
            emphasized,
            leading,
            enabled,
            menu,
            popup,
        } = self;
        let toggled = menu;
        div()
            .relative()
            .flex_none()
            .child(
                row()
                    .id(key)
                    .min_w_0()
                    .h(px(composer::CONTROL_HEIGHT))
                    .px(px(composer::CHIP_HORIZONTAL_PADDING))
                    .gap(px(composer::CHIP_SPACING))
                    .rounded_full()
                    .text_size(px(pane_interior::CAPTION_POINT_SIZE))
                    .font_weight(if emphasized {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(rgb(if emphasized {
                        palette.ink
                    } else {
                        palette.secondary
                    }))
                    .when(enabled, |chip| {
                        chip.cursor_pointer()
                            .hover(|style| style.bg(rgb(palette.hover)))
                    })
                    .when(!enabled, |chip| {
                        chip.opacity(pane_interior::DISABLED_OPACITY)
                    })
                    .own_press()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if enabled {
                            this.toggle_menu(toggled.clone(), cx);
                        }
                    }))
                    .children(leading)
                    .child(
                        div()
                            .min_w_0()
                            .max_w(px(desktop::CHIP_MAX_WIDTH))
                            .truncate()
                            .child(SharedString::from(label.to_owned())),
                    )
                    .child(icon(
                        Icon::ChevronDown,
                        desktop::CHEVRON_GLYPH,
                        palette.secondary,
                    )),
            )
            .children(popup.map(|(body, max_height)| {
                menus::popup_up_to(
                    palette,
                    Anchor::BottomLeft,
                    (0.0, -desktop::POPUP_GAP),
                    max_height,
                    body,
                    cx,
                )
            }))
            .into_any_element()
    }
}

/// The chips in a strip that scrolls sideways when the pane is too narrow for them all.
fn chip_strip(
    scroll: &ScrollHandle,
    pane_id: &str,
    chips: Vec<AnyElement>,
    palette: &Palette,
) -> AnyElement {
    let row = row()
        .flex_none()
        .gap(px(composer::CONTROL_SPACING))
        .children(chips);
    sideways_scroller(id("composer-chips", pane_id), scroll, row, palette)
        .h(px(composer::CONTROL_HEIGHT))
        .into_any_element()
}

/// The narrowest a pane is drawn: the composer's controls (attach, a harness chip showing a
/// short name, the context gauge, the mic and send) side by side, inside the card, the
/// composer's own padding and the pane's inset. Below that the controls would not fit even
/// with the model, effort and permission chips scrolled out of sight. The widths are the
/// composer's design tokens; the label and the percentage are estimated at the width of a
/// character of the tab caption, as the Mac sizes its floor from characters of the agent font.
pub fn minimum_pane_width() -> f32 {
    let character = desktop::TAB_CHARACTER_WIDTH;
    let harness_chip = 2.0 * composer::CHIP_HORIZONTAL_PADDING
        + composer::HARNESS_LOGO_SIZE
        + 2.0 * composer::CHIP_SPACING
        + desktop::PANE_FLOOR_LABEL_CHARACTERS * character
        + desktop::CHEVRON_GLYPH;
    let gauge = composer::RING_DIAMETER
        + composer::CONTEXT_SPACING
        + 4.0 * character
        + 2.0 * composer::CONTEXT_HORIZONTAL_PADDING;
    let controls = [
        composer::CONTROL_HEIGHT,
        harness_chip,
        gauge,
        composer::CONTROL_HEIGHT,
        composer::SEND_WIDTH,
    ];
    let row =
        controls.iter().sum::<f32>() + (controls.len() - 1) as f32 * composer::CONTROL_SPACING;
    // The composer card sits inside the pane's body with its own margin and padding.
    let card = 4.0 * pane_interior::CARD_PADDING;
    row + card + 2.0 * pane_layout::PANE_CARD_INSET
}

fn separator(palette: &Palette) -> AnyElement {
    div()
        .flex_none()
        .text_size(px(pane_interior::CAPTION_POINT_SIZE))
        .text_color(rgb(palette.secondary))
        .child("\u{b7}")
        .into_any_element()
}

/// The context ring and its percentage; empty until the harness reports its context use.
fn context_gauge(controls: &Controls, palette: &Palette) -> AnyElement {
    let used = controls.context.unwrap_or(0.0);
    let color = if used > client_policy::CONTEXT_BAND_DANGER_ABOVE {
        palette.danger()
    } else if used > client_policy::CONTEXT_BAND_WARNING_ABOVE {
        palette.warning()
    } else {
        palette.secondary
    };
    row()
        .flex_none()
        .gap(px(composer::CONTEXT_SPACING))
        .px(px(composer::CONTEXT_HORIZONTAL_PADDING))
        .child(
            div()
                .relative()
                .size(px(composer::RING_DIAMETER))
                .flex_none()
                .child(ring_canvas(
                    controls
                        .context
                        .map(|fraction| (fraction, rgb(color).into())),
                    Some(rgb(palette.well).into()),
                    composer::RING_DIAMETER,
                    composer::RING_LINE_WIDTH,
                )),
        )
        .child(
            div()
                .mono()
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(pane_interior::MONO_SMALL_POINT_SIZE))
                .text_color(rgb(color))
                .child(SharedString::from(controls.context_label())),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_floor_holds_every_fixed_control_and_the_card_around_them() {
        let floor = minimum_pane_width();
        let fixed = 2.0 * composer::CONTROL_HEIGHT + composer::SEND_WIDTH;
        let card = 4.0 * pane_interior::CARD_PADDING + 2.0 * pane_layout::PANE_CARD_INSET;
        assert!(floor > fixed + card, "{floor}");
        // A bounded floor: wider than a phone sliver, narrower than half a small window.
        assert!((300.0..400.0).contains(&floor), "{floor}");
    }
}

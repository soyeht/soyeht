//! The composer of the approved frames: one flat card with the message line on top and one row
//! of controls under it: "+", the harness, model, effort and permission chips, the context ring,
//! the mic and a round send button (stop while the agent runs).

use super::{
    harness_menu::Pick,
    menus,
    ui::{OwnPress, Typeface, column, icon, id, logo, row, sideways_scroller},
    usage_ring::ring_canvas,
};
use crate::{
    assets::{self, Icon},
    open_menu::Menu,
    theme::{Palette, composer, desktop, pane_interior, pane_layout},
    workbench::Workbench,
};
use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    MouseButton, ParentElement, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    div, prelude::FluentBuilder, px, rgb,
};
use soyeht_client::agent::{Action, Controls, Menu as ConfigMenu};
use soyeht_protocol::Pane;
use std::rc::Rc;

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

        let controls_row = match &controls {
            Some(controls) => {
                self.control_row(pane, controls, harness_name.as_deref(), palette, cx)
            }
            None => row().into_any_element(),
        };
        column()
            .flex_none()
            .mx(px(pane_interior::CARD_PADDING))
            .mb(px(pane_interior::CARD_VERTICAL_PADDING))
            .px(px(pane_interior::CARD_PADDING))
            .py(px(pane_interior::CARD_VERTICAL_PADDING))
            .gap(px(composer::ROW_SPACING))
            .rounded(px(pane_interior::COMPOSER_RADIUS))
            .bg(rgb(palette.canvas))
            .text_size(px(pane_interior::BODY_POINT_SIZE))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.focus_input(&focus_id, window, cx)),
            )
            .child(field)
            .child(controls_row)
            .into_any_element()
    }

    fn control_row(
        &mut self,
        pane: &Pane,
        controls: &Controls,
        harness_name: Option<&str>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane.pane_id.clone();
        let connected = self.state.connection == soyeht_client::Connection::Connected;
        let enabled = connected && !controls.busy;
        let harness = pane.agent.as_ref().map(|agent| agent.harness.clone());

        let mut chips: Vec<AnyElement> = Vec::new();
        chips.push(self.harness_chip(
            &pane_id,
            harness.as_deref(),
            harness_name,
            enabled,
            palette,
            cx,
        ));
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
            .child(self.round_button(
                "attach",
                Icon::Plus,
                "Attachments are not available in the desktop client yet.",
                notice_for("attach"),
                palette,
                cx,
            ))
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
            .child(
                row()
                    .id(id("composer", name))
                    .size(px(composer::CONTROL_HEIGHT))
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(palette.hover)))
                    .own_press()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.toggle_menu(toggled.clone(), cx)
                    }))
                    .child(icon(glyph, desktop::COMPOSER_GLYPH, palette.secondary)),
            )
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
        row()
            .id(id("composer-send", &pane_id))
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

    fn harness_chip(
        &mut self,
        pane_id: &str,
        current: Option<&str>,
        name: Option<&str>,
        enabled: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let menu = Menu::Harness(pane_id.to_owned());
        let open = self.menus.is_open(&menu);
        let label = name.or(current).unwrap_or("Agent").to_owned();
        let mark = current
            .and_then(assets::logo_path)
            .map(|path| logo(path, composer::HARNESS_LOGO_SIZE, palette.ink).into_any_element());
        let rows = if open {
            let pane_id = pane_id.to_owned();
            let pick: Pick =
                Rc::new(move |this, harness, cx| this.switch_harness(&pane_id, harness, cx));
            self.harness_rows(current, palette, &pick, cx)
        } else {
            Vec::new()
        };
        chip(
            palette,
            id("harness-chip", pane_id),
            &label,
            true,
            mark,
            enabled,
            menu,
            open.then(|| menus::list(rows)),
            cx,
        )
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
        chip(
            palette,
            id("config-chip", &format!("{pane_id}-{}", menu.config_id)),
            menu.label(),
            false,
            leading.map(|glyph| {
                icon(glyph, composer::SHIELD_SIZE, palette.secondary).into_any_element()
            }),
            enabled,
            open_menu,
            open.then(|| menus::list(rows)),
            cx,
        )
    }
}

/// A borderless chip of the control row: an optional mark, the label and a chevron.
#[allow(clippy::too_many_arguments)]
fn chip(
    palette: &'static Palette,
    key: SharedString,
    label: &str,
    emphasized: bool,
    leading: Option<AnyElement>,
    enabled: bool,
    menu: Menu,
    popup: Option<Div>,
    cx: &mut Context<Workbench>,
) -> AnyElement {
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
        .children(popup.map(|body| {
            menus::popup(
                palette,
                Anchor::BottomLeft,
                (0.0, -desktop::POPUP_GAP),
                body,
                cx,
            )
        }))
        .into_any_element()
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
    let color = if used > 0.9 {
        palette.danger()
    } else if used > 0.75 {
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

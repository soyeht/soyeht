//! The Harnesses panel of Preferences, as the Mac's: a raised row for each harness the engine
//! lists with its version, the model, effort and permission mode a new pane starts in, and how
//! ready it is. A harness that can start can be made the default, and its permission mode can be
//! chosen before any pane runs it.

use super::{
    menus,
    preferences::{page, pill_button},
    ui::{OwnPress, Typeface, column, icon, id, row},
};
use crate::{
    assets::Icon,
    neo,
    prefs::rows_wrap,
    theme::{Palette, desktop, elevation, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, FontWeight, SharedString, div, prelude::*, px,
    rgb,
};
use soyeht_client::{
    catalogue::{self, Catalogue},
    harness_picker::{self, Group},
};
use soyeht_protocol::{ConfigOption, ConfigValue, HarnessInfo};

const SUMMARY: &str = "Each harness runs as an engine subprocess and communicates over ACP. A raw terminal remains available for every command-line agent.";
const NO_MODE: &str = "Harness default";

impl Workbench {
    pub(super) fn harnesses_page(
        &mut self,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let actions = row().gap(px(space::X10)).child(
            pill_button(palette, "rescan".into(), Some(Icon::Refresh), "Rescan")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.rescan_harnesses(cx))),
        );
        let harnesses = self.state.harnesses().to_vec();
        let preferred = self.state.preferences().preferred_harness.clone();
        let default = harness_picker::default_harness(&harnesses, None, preferred.as_deref())
            .map(|harness| harness.id.clone());
        let rows: Vec<AnyElement> = harnesses
            .iter()
            .map(|harness| {
                let catalogue = self.state.catalogue(&harness.id).cloned();
                let mode = self
                    .state
                    .preferences()
                    .default_modes
                    .get(&harness.id)
                    .cloned();
                self.harness_row(
                    harness,
                    catalogue.as_ref(),
                    default.as_deref() == Some(harness.id.as_str()),
                    mode.as_deref(),
                    palette,
                    cx,
                )
            })
            .collect();
        let content = if rows.is_empty() {
            column().child(menus::note(
                palette,
                if self.state.harnesses_loaded() {
                    "No agents available. Rescan after installing or configuring an agent."
                } else {
                    "Loading harnesses\u{2026}"
                },
            ))
        } else {
            column().gap(px(space::X10)).children(rows)
        };
        page(palette, "Harnesses", Some(SUMMARY), Some(actions), content)
    }

    fn harness_row(
        &mut self,
        harness: &HarnessInfo,
        catalogue: Option<&Catalogue>,
        is_default: bool,
        default_mode: Option<&str>,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let options: &[ConfigOption] = catalogue.map_or(harness.options.as_slice(), |catalogue| {
            catalogue.options.as_slice()
        });
        let modes = options
            .iter()
            .find(|option| option.category.as_deref() == Some("mode") || option.id == "mode");
        let mode_label = modes
            .and_then(|option| {
                option
                    .options
                    .iter()
                    .find(|choice| Some(choice.value.as_str()) == default_mode)
            })
            .map_or(NO_MODE, |choice| choice.name.as_str());
        let open = self.prefs.status_open.as_deref() == Some(harness.id.as_str());
        let version = match harness
            .version
            .as_deref()
            .filter(|version| !version.is_empty())
        {
            Some(version) => format!("{version} \u{b7} ACP"),
            None => "Version unavailable \u{b7} ACP".to_owned(),
        };
        let wrap = rows_wrap(self.prefs.detail_width);
        let chips = vec![
            chip(palette, Icon::Cpu, &model_label(options, catalogue)),
            chip(palette, Icon::Gauge, &effort_label(options)),
            self.mode_chip(harness, modes, mode_label, palette, cx),
        ];
        let name = identity(harness, &version, is_default, wrap, palette);
        let status = self.status_button(harness, palette, cx);
        let surface = neo::raised(palette, radius::CONTROL, elevation::TASK_CARD)
            .min_h(px(desktop::HARNESS_ROW_HEIGHT))
            .flex_none()
            .flex()
            .px(px(space::X16));
        // Side by side while the page has room; otherwise the chips go under the name and the
        // status keeps its column, so that nothing ever overlaps.
        let line = if wrap {
            surface
                .flex_col()
                .justify_center()
                .gap(px(space::X8))
                .py(px(space::X10))
                .child(row().gap(px(space::X14)).child(name).child(status))
                .child(row().flex_wrap().gap(px(space::X8)).children(chips))
        } else {
            surface
                .flex_row()
                .items_center()
                .gap(px(space::X14))
                .child(name)
                .child(row().flex_1().min_w_0().gap(px(space::X8)).children(chips))
                .child(status)
        };
        let mut wrapper = column().gap(px(space::X8)).child(line);
        if open {
            wrapper = wrapper.child(self.status_detail(harness, is_default, palette, cx));
        }
        wrapper.into_any_element()
    }

    /// The chip of the permission mode new panes start in: it opens the list of modes the harness
    /// announced, with "Harness default" first.
    fn mode_chip(
        &mut self,
        harness: &HarnessInfo,
        modes: Option<&ConfigOption>,
        label: &str,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.prefs.mode_menu.as_deref() == Some(harness.id.as_str());
        let (toggled, has_modes) = (harness.id.clone(), modes.is_some());
        let current = self
            .state
            .preferences()
            .default_modes
            .get(&harness.id)
            .cloned();
        let popup = (open && modes.is_some()).then(|| {
            let default = menus::on_pick(
                menus::item(palette, "default", NO_MODE, current.is_none()),
                cx,
                {
                    let harness = harness.id.clone();
                    move |this, cx| this.set_default_mode(&harness, None, cx)
                },
            );
            let choices = modes
                .into_iter()
                .flat_map(|option| option.options.iter())
                .map(|choice| {
                    let (harness, value) = (harness.id.clone(), choice.value.clone());
                    menus::on_pick(
                        menus::item(
                            palette,
                            &choice.value,
                            choice.name.clone(),
                            current.as_deref() == Some(choice.value.as_str()),
                        ),
                        cx,
                        move |this, cx| this.set_default_mode(&harness, Some(value.clone()), cx),
                    )
                });
            menus::list(std::iter::once(default).chain(choices).collect())
        });
        div()
            .relative()
            .flex_none()
            .child(
                chip_frame(palette, Icon::Shield, label, true)
                    .id(id("default-mode", &harness.id))
                    .when_some(modes, |chip, _| chip.cursor_pointer())
                    .own_press()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if has_modes {
                            this.toggle_mode_menu(&toggled, cx);
                        }
                    })),
            )
            .children(popup.map(|body| {
                menus::popup(
                    palette,
                    Anchor::TopLeft,
                    (0.0, desktop::HARNESS_ROW_HEIGHT / 2.0),
                    body,
                    cx,
                )
            }))
            .into_any_element()
    }

    /// How ready the harness is: a dot and a word, which open the explanation.
    fn status_button(
        &mut self,
        harness: &HarnessInfo,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = harness_picker::group_of(harness);
        let color = match group {
            Group::Ready => palette.success(),
            Group::SignIn | Group::CheckSetup => palette.warning(),
            Group::NotChecked | Group::NotInstalled => palette.muted,
        };
        let target = harness.id.clone();
        row()
            .id(id("harness-status", &harness.id))
            .w(px(desktop::HARNESS_ROW_STATUS_WIDTH))
            .flex_none()
            .gap(px(space::X6))
            .cursor_pointer()
            .own_press()
            .on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_status(&target, cx)),
            )
            .child(
                div()
                    .size(px(space::X7))
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(color)),
            )
            .child(
                div()
                    .chrome()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(type_scale::CONTROL))
                    .text_color(rgb(palette.secondary))
                    .child(harness_picker::status_label(harness)),
            )
            .into_any_element()
    }

    /// The explanation under a row: what the status means and, for a harness that can start,
    /// making it the default.
    fn status_detail(
        &mut self,
        harness: &HarnessInfo,
        is_default: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = harness.id.clone();
        column()
            .gap(px(space::X8))
            .px(px(space::X16))
            .pb(px(space::X4))
            .child(
                div()
                    .chrome()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(type_scale::CAPTION))
                    .child(harness_picker::status_label(harness)),
            )
            .child(
                div()
                    .chrome()
                    .text_size(px(type_scale::CAPTION))
                    .text_color(rgb(palette.secondary))
                    .child(harness_picker::status_reason(harness)),
            )
            .children((harness.available && !is_default).then(|| {
                row().child(
                    pill_button(
                        palette,
                        id("make-default", &harness.id),
                        None,
                        "Make default",
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| this.make_default_harness(&target, cx),
                    )),
                )
            }))
            .into_any_element()
    }
}

/// The harness's name, a "default" mark when it is the one new agents start with, and its version.
fn identity(
    harness: &HarnessInfo,
    version: &str,
    is_default: bool,
    wrap: bool,
    palette: &Palette,
) -> Div {
    let width = if wrap {
        column().flex_1().min_w_0()
    } else {
        column()
            .w(px(desktop::HARNESS_ROW_IDENTITY_WIDTH))
            .flex_none()
    };
    width
        .gap(px(space::X2))
        .child(
            row()
                .gap(px(space::X8))
                .child(
                    div()
                        .chrome()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(desktop::PREFERENCES_ROW_TITLE_SIZE))
                        .child(SharedString::from(harness.name.clone())),
                )
                .children(is_default.then(|| {
                    div()
                        .flex_none()
                        .px(px(space::X7))
                        .py(px(desktop::HAIRLINE))
                        .rounded_full()
                        .bg(rgb(palette.well))
                        .chrome()
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(type_scale::MICRO))
                        .text_color(rgb(palette.secondary))
                        .child("default")
                })),
        )
        .child(
            div()
                .mono()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(type_scale::MICRO))
                .text_color(rgb(palette.muted))
                .child(SharedString::from(version.to_owned())),
        )
}

/// The model a new pane starts in: the option's current choice, else the first the catalogue
/// lists, else the harness's own.
fn model_label(options: &[ConfigOption], catalogue: Option<&Catalogue>) -> String {
    options
        .iter()
        .find(|option| option.category.as_deref() == Some("model"))
        .and_then(current_choice)
        .or_else(|| {
            catalogue
                .and_then(|catalogue| catalogue.models.first())
                .map(|model| model.name.clone())
        })
        .unwrap_or_else(|| "Agent default".to_owned())
}

fn effort_label(options: &[ConfigOption]) -> String {
    catalogue::effort_of(options)
        .and_then(current_choice)
        .unwrap_or_else(|| "Default effort".to_owned())
}

/// The name of the choice an option is set to; its raw value when it names none.
fn current_choice(option: &ConfigOption) -> Option<String> {
    let ConfigValue::V0(value) = &option.current_value else {
        return None;
    };
    Some(
        option
            .options
            .iter()
            .find(|choice| &choice.value == value)
            .map_or_else(|| value.clone(), |choice| choice.name.clone()),
    )
}

/// A raised chip with an icon and a label, as the Mac's `PreferenceChip`.
fn chip(palette: &Palette, glyph: Icon, label: &str) -> AnyElement {
    chip_frame(palette, glyph, label, true).into_any_element()
}

fn chip_frame(palette: &Palette, glyph: Icon, label: &str, chevron: bool) -> Div {
    neo::raised(palette, radius::PILL, elevation::CHIP)
        .h(px(desktop::PREFERENCES_CHIP_HEIGHT))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(space::X5))
        .px(px(space::X8))
        .chrome()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(type_scale::CONTROL))
        .text_color(rgb(palette.secondary))
        .child(icon(glyph, desktop::CHEVRON_GLYPH + 1.0, palette.secondary))
        .child(
            div()
                .max_w(px(desktop::CHIP_MAX_WIDTH))
                .truncate()
                .child(SharedString::from(label.to_owned())),
        )
        .children(chevron.then(|| icon(Icon::ChevronDown, desktop::CHEVRON_GLYPH, palette.muted)))
}

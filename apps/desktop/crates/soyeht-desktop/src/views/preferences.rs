//! Preferences, from the rail's gear: a raised panel over the window with the panels down the
//! left and the open one beside them, as the Mac's sheet is. Each panel is a flat page on the
//! panel's face; only the panel itself and the rows and chips on it are raised.

use super::ui::{OwnPress, Typeface, chrome_text, column, icon, id, row};
use crate::{
    assets::Icon,
    build_info, neo,
    prefs::{Section, page_width, sheet_extent},
    theme::{Palette, desktop, elevation, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, Pixels, SharedString, Size, div,
    prelude::*, px, rgb,
};
use soyeht_client::Connection;

impl Workbench {
    /// The Preferences panel over the whole window, `window` points big.
    pub(super) fn preferences_overlay(
        &mut self,
        palette: &'static Palette,
        window: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (width, height) = sheet_extent((f32::from(window.width), f32::from(window.height)));
        self.prefs.detail_width = page_width(width);
        let section = self.prefs.section();
        let detail = match section {
            Section::Setup => self.setup_page(palette, cx),
            Section::Appearance => self.appearance_page(palette, cx),
            Section::Harnesses => self.harnesses_page(palette, cx),
            Section::Usage => self.usage_page(palette, cx),
            Section::Skills => self.skills_page(palette, cx),
        };
        let sidebar = self.sidebar(section, palette, cx);
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                neo::raised(palette, radius::CARD, elevation::PANEL)
                    .id("preferences")
                    .w(px(width))
                    .h(px(height))
                    .flex()
                    .flex_row()
                    .gap(px(space::X10))
                    .p(px(space::X10))
                    .text_color(rgb(palette.ink))
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_menu(cx)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(sidebar)
                    .child(
                        div()
                            .id("preferences-detail")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .rounded(px(radius::CONTROL))
                            .bg(rgb(palette.face))
                            .overflow_y_scroll()
                            .child(detail),
                    ),
            )
            .into_any_element()
    }

    /// The panels down the left, and the build and the engine at the foot.
    fn sidebar(
        &mut self,
        selected: Section,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let items: Vec<AnyElement> = Section::ALL
            .into_iter()
            .map(|section| {
                let current = section == selected;
                row()
                    .id(id("preferences-section", section.title()))
                    .h(px(desktop::PREFERENCES_SIDEBAR_ROW_HEIGHT))
                    .gap(px(space::X10))
                    .px(px(space::X12))
                    .rounded(px(radius::COMPACT))
                    .cursor_pointer()
                    .when(current, |item| item.bg(rgb(palette.well)))
                    .when(!current, |item| {
                        item.hover(|style| style.bg(rgb(palette.hover)))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.select_section(section, cx)
                    }))
                    .child(icon(
                        section_icon(section),
                        desktop::PREFERENCES_GLYPH,
                        if current {
                            palette.accent
                        } else {
                            palette.secondary
                        },
                    ))
                    .child(
                        div()
                            .chrome()
                            .text_size(px(type_scale::CAPTION))
                            .font_weight(if current {
                                FontWeight::BOLD
                            } else {
                                FontWeight::SEMIBOLD
                            })
                            .text_color(rgb(if current {
                                palette.ink
                            } else {
                                palette.secondary
                            }))
                            .child(section.title()),
                    )
                    .into_any_element()
            })
            .collect();
        let engine = if self.state.connection == Connection::Connected {
            "Engine connected"
        } else {
            "Engine reconnecting"
        };
        column()
            .w(px(desktop::PREFERENCES_SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .rounded(px(radius::CONTROL))
            .bg(rgb(palette.face))
            .py(px(space::X12))
            .px(px(space::X8))
            .gap(px(space::X4))
            .child(
                div()
                    .chrome()
                    .px(px(space::X4))
                    .pb(px(space::X8))
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(desktop::PREFERENCES_TITLE_SIZE))
                    .child("Preferences"),
            )
            .children(items)
            .child(div().flex_1())
            .child(
                column()
                    .px(px(space::X12))
                    .py(px(space::X8))
                    .gap(px(space::X2))
                    .mono()
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(px(type_scale::MICRO))
                    .text_color(rgb(palette.muted))
                    .child(format!("Soyeht {}", build_info::id()))
                    .children(build_info::profile_note())
                    .child(format!("{engine} \u{b7} ACP v1")),
            )
    }
}

fn section_icon(section: Section) -> Icon {
    match section {
        Section::Setup => Icon::ListChecks,
        Section::Appearance => Icon::Palette,
        Section::Harnesses => Icon::Cpu,
        Section::Usage => Icon::Gauge,
        Section::Skills => Icon::Skills,
    }
}

/// A page of Preferences: its title with the actions that belong to it at the right, the
/// sentence that says what it is, then its content.
pub(super) fn page(
    palette: &Palette,
    title: &str,
    summary: Option<&str>,
    actions: Option<Div>,
    content: Div,
) -> Div {
    column()
        .w_full()
        .gap(px(space::X16))
        .py(px(space::X16))
        .px(px(desktop::PREFERENCES_SIDE_PADDING))
        .child(
            row()
                .gap(px(space::X10))
                .child(
                    chrome_text(FontWeight::BOLD, desktop::PREFERENCES_PAGE_TITLE_SIZE)
                        .flex_1()
                        .child(SharedString::from(title.to_owned())),
                )
                .children(actions),
        )
        .children(summary.map(|summary| {
            div()
                .chrome()
                .text_size(px(type_scale::CAPTION))
                .text_color(rgb(palette.secondary))
                .child(SharedString::from(summary.to_owned()))
        }))
        .child(content)
}

/// A raised pill button with an optional icon, as the Mac's `NeoPillButtonStyle`.
pub(super) fn pill_button(
    palette: &Palette,
    key: SharedString,
    glyph: Option<Icon>,
    label: &str,
) -> gpui::Stateful<Div> {
    neo::raised(palette, radius::PILL, elevation::PILL)
        .id(key)
        .flex()
        .items_center()
        .px(px(space::X14))
        .py(px(space::X8))
        .gap(px(space::X6))
        .chrome()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(type_scale::CONTROL))
        .text_color(rgb(palette.ink))
        .cursor_pointer()
        .active(|style| style.opacity(crate::theme::pane_interior::PRESSED_OPACITY))
        .own_press()
        .children(glyph.map(|glyph| icon(glyph, desktop::INLINE_GLYPH, palette.ink)))
        .child(SharedString::from(label.to_owned()))
}

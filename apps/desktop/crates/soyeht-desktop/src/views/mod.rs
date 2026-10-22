//! Rendering of the workbench, laid out as the Mac window is: a top bar of workspace tabs, the
//! rail down the left, and the panes of the selected workspace as neumorphic cards.
//!
//! Only chrome and the pane shell are raised; the inside of a pane is flat. Every color is a
//! role of the active palette and every size is a design token.

mod agent;
mod attachments;
mod cards;
mod composer;
mod copy_button;
mod editor;
mod editor_find;
mod editor_tabs;
mod editor_tree;
mod harness_menu;
mod loop_panel;
mod markdown;
pub(crate) mod menus;
mod pane;
mod permission;
mod preferences;
mod prefs_harnesses;
mod prefs_skills;
mod prefs_usage;
mod prelude;
mod rail;
mod readiness;
mod resize_handles;
pub(crate) mod selectable;
mod settings;
mod switch_popover;
mod tasks;
mod terminal_menu;
mod tiles;

pub(crate) use tiles::sizing;
mod top_bar;
mod ui;
mod usage_ring;
mod welcome;
mod window_controls;

use crate::{
    assets::Icon,
    theme::{desktop, fonts, glyph, pane_layout, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
    relative, rgb,
};
use soyeht_client::Connection;

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_focus_request(window, cx);
        self.settle_rename(window, cx);
        self.window_height = f32::from(window.viewport_size().height);
        self.window_width = f32::from(window.viewport_size().width);
        let palette = self.theme.palette();
        div()
            .id("workbench")
            .key_context("Workbench")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::open_folder_action))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy_selection))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(palette.canvas))
            .text_color(rgb(palette.ink))
            .font_family(fonts::UI)
            .line_height(relative(desktop::LINE_HEIGHT_UI))
            .text_size(px(type_scale::CAPTION))
            // The Mac's top inset: the bar floats this far under the window's edge.
            .pt(px(space::X8))
            .child(self.top_bar(palette, window, cx))
            .children(self.banners(palette, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    // The open rail starts in the screen padding, and the first pane follows it
                    // after the grid's padding (Tela 12); without the rail the panes keep the
                    // whole margin.
                    .when(self.rail_open, |row| {
                        row.pl(px(pane_layout::SCREEN_PADDING))
                    })
                    .children(self.rail_open.then(|| self.rail(palette, cx)))
                    .child(self.stage(palette, cx)),
            )
            .children(
                self.prefs_open()
                    .then(|| self.preferences_overlay(palette, window.viewport_size(), cx)),
            )
            .children(resize_handles::handles(window))
    }
}

impl Workbench {
    /// The warning lines under the top bar, each until the person dismisses it: a request that
    /// failed, then the client's standing note (a software renderer), which a connection does
    /// not clear.
    fn banners(
        &mut self,
        palette: &'static crate::theme::Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        type Dismiss = fn(&mut soyeht_client::state::State);
        let lines: [(&'static str, Option<String>, Dismiss); 2] = [
            ("problem", self.state.problem.clone(), |state| {
                state.problem = None;
            }),
            ("notice", self.state.notice.clone(), |state| {
                state.notice = None;
            }),
        ];
        lines
            .into_iter()
            .filter_map(|(id, text, dismiss)| Some(banner_line(id, text?, palette, cx, dismiss)))
            .collect()
    }

    /// The panes of the selected workspace, tiled as its layout says. The outermost panes keep
    /// the margin the design gives them from the window and the top bar (24); beside the open
    /// rail the first one follows it after the grid's padding.
    fn stage(&mut self, palette: &'static crate::theme::Palette, cx: &mut Context<Self>) -> Div {
        let outer = pane_layout::SCREEN_PADDING + pane_layout::GRID_PADDING;
        let stage = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .pt(px(outer))
            .pb(px(outer))
            .pr(px(outer))
            .pl(px(if self.rail_open {
                pane_layout::GRID_PADDING
            } else {
                outer
            }));
        stage.child(self.tiles(palette, cx))
    }
}

/// One warning line under the top bar; a click dismisses it through `dismiss`.
fn banner_line(
    id: &'static str,
    text: String,
    palette: &'static crate::theme::Palette,
    cx: &mut Context<Workbench>,
    dismiss: fn(&mut soyeht_client::state::State),
) -> AnyElement {
    ui::row()
        .id(id)
        .flex_none()
        .gap(px(space::X8))
        .px(px(pane_layout::SCREEN_PADDING + pane_layout::GRID_PADDING))
        .text_size(px(type_scale::CONTROL))
        .text_color(rgb(palette.warning()))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            dismiss(&mut this.state);
            cx.notify();
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(SharedString::from(text)),
        )
        .child(ui::icon(Icon::Close, glyph::TAB_CLOSE, palette.warning()))
        .into_any_element()
}

/// What the stage says when the connection is not up, and so there is nothing else to show. A
/// live connection with no workspace shows the welcome instead.
pub(crate) fn connection_hint(connection: &Connection) -> Option<&'static str> {
    match connection {
        Connection::Connecting => Some("Connecting to the engine\u{2026}"),
        Connection::Starting => Some("Starting the engine\u{2026}"),
        Connection::Connected => None,
        Connection::Disconnected(_) => {
            Some("The engine is not running. Start it and this window reconnects.")
        }
    }
}

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
mod diagnostics;
mod dividers;
mod editor;
mod editor_find;
mod editor_preview;
mod editor_tabs;
mod editor_tree;
mod harness_menu;
mod loop_panel;
mod markdown;
pub(crate) use markdown::Replies;
pub(crate) mod menus;
pub(crate) mod overlay;
mod pane;
mod pane_drag;
mod permission;
mod places;
mod preferences;
mod prefs_harnesses;
mod prefs_skills;
mod prefs_usage;
mod prelude;
mod presentation;
mod rail;
mod readiness;
mod resize_handles;
pub(crate) mod selectable;
mod settings;
mod slash;
mod start;
mod switch_popover;
mod tasks;
mod terminal_menu;
mod tiles;
mod web;

pub(crate) use tiles::sizing;
pub(crate) use ui::is_turn_down;

mod top_bar;
mod ui;
mod updates;
mod usage_ring;
mod welcome;
mod window_controls;
mod writing;

use crate::{
    assets::Icon,
    theme::{desktop, fonts, glyph, pane_layout, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px, relative, rgb,
};
use soyeht_client::{Connection, state::problem::Banner};

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_focus_request(window, cx);
        self.settle_preedit(window, cx);
        self.settle_rename(window, cx);
        self.window.height = f32::from(window.viewport_size().height);
        self.window.width = f32::from(window.viewport_size().width);
        self.web_shown.clear();
        menus::window_measured(self.window.height, cx);
        overlay::begin_frame(cx);
        let palette = self.theme.palette();
        let screen = div()
            .id("workbench")
            .key_context("Workbench")
            .track_focus(&self.focus)
            // Escape cancels the drag of a pane before the pane in front takes the key.
            .capture_key_down(cx.listener(Self::pane_drag_key))
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_up(MouseButton::Left, pane_drag::ended(cx))
            .on_mouse_up_out(MouseButton::Left, pane_drag::ended(cx))
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::open_folder_action))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::create_task_key))
            .on_action(cx.listener(Self::create_and_dispatch_task_key))
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
            .children(self.banner(palette, cx))
            .children(self.workspace_close_question(cx))
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
            .child(crate::workbench::input_surface(&self.focus, cx));
        // A page that is a window of its own stays where it was until it is told to go.
        self.hide_unshown_web();
        screen
    }
}

impl Workbench {
    /// The warning line under the top bar, until the person dismisses it. One line at a time, as
    /// the Mac shows one failure at a time: a request that failed, then, once that is dismissed,
    /// the client's standing note (a software renderer), which a connection does not clear.
    fn banner(
        &mut self,
        palette: &'static crate::theme::Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        Some(match self.state.banner()? {
            Banner::Problem(text) => {
                let text = text.to_owned();
                banner_line("problem", text, "Dismiss", palette, cx, |this, cx| {
                    this.state.dismiss_problem();
                    cx.notify();
                })
            }
            Banner::Notice(text) => {
                let text = text.to_owned();
                banner_line(
                    "notice",
                    text,
                    "Dismiss for good",
                    palette,
                    cx,
                    |this, cx| this.dismiss_notice(cx),
                )
            }
        })
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

/// One warning line under the top bar; a click dismisses it through `dismiss`, and `tip` says
/// what the dismissal does.
fn banner_line(
    id: &'static str,
    text: String,
    tip: &'static str,
    palette: &'static crate::theme::Palette,
    cx: &mut Context<Workbench>,
    dismiss: fn(&mut Workbench, &mut Context<Workbench>),
) -> AnyElement {
    ui::row()
        .id(id)
        .flex_none()
        .gap(px(space::X8))
        .px(px(pane_layout::SCREEN_PADDING + pane_layout::GRID_PADDING))
        .text_size(px(type_scale::CONTROL))
        .text_color(rgb(palette.warning()))
        .cursor_pointer()
        .tooltip(ui::hint(tip, palette))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| dismiss(this, cx)))
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

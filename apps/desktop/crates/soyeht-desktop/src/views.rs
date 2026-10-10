//! Rendering of the workbench: a workspace rail and the panes of the selected workspace.
//!
//! Only the pane shell is raised; the rail, the headers and the terminal inside a pane are
//! flat. Every color is a role of the active palette.

use crate::{
    fonts::{UI_FAMILY, UI_SIZE},
    pane_shell::{self, CORNER_RADIUS, HALO},
    terminal_view,
    theme::Palette,
    workbench::{NewTerminal, Workbench},
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px, rgb,
};
use soyeht_client::Connection;
use soyeht_protocol::{Pane, PaneMode, PaneState, Workspace};

/// Gap between panes: the shadow halo plus a margin, so neighbours never clip each other.
const PANE_GAP: f32 = HALO + 9.0;
const RAIL_WIDTH: f32 = 244.0;
const HEADER_HEIGHT: f32 = 44.0;
/// Clears the window buttons that sit over the top of the rail.
const RAIL_TOP: f32 = 48.0;

impl Render for Workbench {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.theme.palette();
        div()
            .id("workbench")
            .key_context("Workbench")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::paste))
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(palette.canvas))
            .text_color(rgb(palette.ink))
            .font_family(UI_FAMILY)
            .text_size(px(UI_SIZE))
            .child(self.rail(palette, cx))
            .child(self.stage(palette, cx))
    }
}

impl Workbench {
    fn rail(&self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let selected = self
            .state
            .selected_workspace()
            .map(|w| w.workspace_id.as_str());
        let rows: Vec<AnyElement> = self
            .state
            .workspaces()
            .iter()
            .map(|workspace| {
                let active = selected == Some(workspace.workspace_id.as_str());
                self.workspace_row(workspace, active, palette, cx)
            })
            .collect();
        let connected = self.state.connection == Connection::Connected;
        div()
            .w(px(RAIL_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .px(px(20.0))
            .pb(px(20.0))
            .pt(px(RAIL_TOP))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(17.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Soyeht"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(if connected {
                                palette.secondary
                            } else {
                                palette.warning()
                            }))
                            .child(connection_text(&self.state.connection)),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette.muted))
                    .child("WORKSPACES"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap(px(4.0))
                    .children(rows),
            )
            .child(self.new_terminal_button(palette, connected, cx))
            .children(self.state.problem.as_ref().map(|problem| {
                div()
                    .text_size(px(12.0))
                    .text_color(rgb(palette.danger()))
                    .child(SharedString::from(problem.clone()))
            }))
    }

    fn workspace_row(
        &self,
        workspace: &Workspace,
        active: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = workspace.workspace_id.clone();
        let panes = self.state.panes_of(&id).count();
        let background = if active { palette.well } else { palette.canvas };
        div()
            .id(SharedString::from(format!("workspace-{id}")))
            .flex()
            .flex_col()
            .gap(px(1.0))
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(10.0))
            .bg(rgb(background))
            .hover(|style| style.bg(rgb(if active { palette.well } else { palette.hover })))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.select_workspace(&id, cx);
                window.focus(&this.focus, cx);
            }))
            .child(
                div()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .child(SharedString::from(workspace.name.clone())),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(rgb(palette.secondary))
                    .child(SharedString::from(format!(
                        "{} \u{b7} {} pane{}",
                        leaf(&workspace.path),
                        panes,
                        if panes == 1 { "" } else { "s" }
                    ))),
            )
            .into_any_element()
    }

    /// The primary button: ink on the face color, flat.
    fn new_terminal_button(
        &self,
        palette: &'static Palette,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_workspace = self.state.selected_workspace().is_some();
        let active = enabled && has_workspace;
        div()
            .id("new-terminal")
            .flex()
            .justify_center()
            .py(px(9.0))
            .rounded(px(10.0))
            .font_weight(FontWeight::MEDIUM)
            .bg(rgb(if active { palette.ink } else { palette.well }))
            .text_color(rgb(if active { palette.face } else { palette.muted }))
            .when(active, |button| button.cursor_pointer())
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                if active {
                    this.new_terminal(&NewTerminal, window, cx);
                }
            }))
            .child("New terminal")
    }

    fn stage(&self, palette: &'static Palette, cx: &mut Context<Self>) -> Div {
        let stage = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .p(px(PANE_GAP))
            .flex()
            .flex_col()
            .gap(px(PANE_GAP));
        let Some(workspace) = self.state.selected_workspace() else {
            return stage.child(empty_state(palette, &self.state.connection));
        };
        let panes: Vec<&Pane> = self.state.panes_of(&workspace.workspace_id).collect();
        if panes.is_empty() {
            return stage.child(empty_message(
                palette,
                "This workspace has no panes yet. Use New terminal.",
            ));
        }
        let focused = self.state.focused_pane().map(|pane| pane.pane_id.as_str());
        let columns = columns_for(panes.len());
        let mut rows = Vec::new();
        for chunk in panes.chunks(columns) {
            let cards: Vec<AnyElement> = chunk
                .iter()
                .map(|pane| {
                    self.pane_card(pane, focused == Some(pane.pane_id.as_str()), palette, cx)
                })
                .collect();
            rows.push(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .gap(px(PANE_GAP))
                    .children(cards),
            );
        }
        stage.children(rows)
    }

    fn pane_card(
        &self,
        pane: &Pane,
        focused: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = pane.pane_id.clone();
        let focus_id = id.clone();
        let body = match pane.mode {
            PaneMode::Terminal => self.terminal_body(&pane.pane_id, focused, palette, cx),
            _ => empty_message(palette, mode_notice(&pane.mode)).into_any_element(),
        };
        pane_shell::shell(palette)
            .id(SharedString::from(format!("pane-{id}")))
            .flex_1()
            .min_w_0()
            .min_h_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.focus_pane(&focus_id, window, cx)),
            )
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .rounded(px(CORNER_RADIUS))
                    .overflow_hidden()
                    .child(self.pane_header(pane, focused, palette, cx))
                    .child(body),
            )
            .into_any_element()
    }

    fn pane_header(
        &self,
        pane: &Pane,
        focused: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = pane.pane_id.clone();
        let detail = pane_detail(pane, self.state.has_gap(&pane.pane_id));
        div()
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .px(px(18.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .size(px(8.0))
                    .rounded_full()
                    .bg(rgb(state_color(&pane.state, palette))),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(if focused {
                        palette.ink
                    } else {
                        palette.secondary
                    }))
                    .child(SharedString::from(pane.name.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(rgb(palette.muted))
                    .child(SharedString::from(detail)),
            )
            .child(
                div()
                    .id(SharedString::from(format!("close-{id}")))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .text_color(rgb(palette.muted))
                    .hover(|style| style.bg(rgb(palette.hover)).text_color(rgb(palette.ink)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, _| this.close_pane(&id)))
                    .child("\u{d7}"),
            )
    }

    fn terminal_body(
        &self,
        pane_id: &str,
        focused: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = pane_id.to_owned();
        div()
            .id(SharedString::from(format!("terminal-{id}")))
            .flex_1()
            .min_h_0()
            .px(px(18.0))
            .pb(px(16.0))
            .on_scroll_wheel(cx.listener({
                let id = id.clone();
                move |this, event, _, cx| this.scroll_terminal(&id, event, cx)
            }))
            .child(terminal_view::surface(cx.entity(), id, palette, focused))
            .into_any_element()
    }
}

fn connection_text(connection: &Connection) -> SharedString {
    match connection {
        Connection::Connecting => "Connecting to the engine\u{2026}".into(),
        Connection::Connected => "Connected".into(),
        Connection::Disconnected(reason) => format!("Engine unavailable \u{b7} {reason}").into(),
    }
}

fn empty_state(palette: &'static Palette, connection: &Connection) -> Div {
    let message = match connection {
        Connection::Connected => "No workspaces yet. Create one with the soyeht command line.",
        Connection::Connecting => "Connecting to the engine\u{2026}",
        Connection::Disconnected(_) => {
            "The engine is not running. Start it and this window reconnects."
        }
    };
    empty_message(palette, message)
}

fn empty_message(palette: &'static Palette, message: &str) -> Div {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .px(px(24.0))
        .text_color(rgb(palette.muted))
        .child(SharedString::from(message.to_owned()))
}

fn mode_notice(mode: &PaneMode) -> &'static str {
    match mode {
        PaneMode::Agent => "Agent panes are not available in the desktop client yet.",
        PaneMode::Editor => "Editor panes are not available in the desktop client yet.",
        PaneMode::Web => "Web panes are not available in the desktop client yet.",
        PaneMode::Writing => "Writing panes are not available in the desktop client yet.",
        PaneMode::Terminal => "",
    }
}

/// The line under a pane's name. The two notices mirror `TerminalStatePresentation`.
fn pane_detail(pane: &Pane, has_gap: bool) -> String {
    match pane.state {
        PaneState::Dead => "Terminal process exited.".into(),
        _ if has_gap => "Some output was not recorded. The screen contains a gap.".into(),
        _ => pane.phrase.clone(),
    }
}

fn state_color(state: &PaneState, palette: &Palette) -> u32 {
    match state {
        PaneState::Working | PaneState::Starting | PaneState::Recovering => palette.accent,
        PaneState::WaitingForYou | PaneState::NoSignal | PaneState::Cancelling => palette.warning(),
        PaneState::Dead => palette.danger(),
        PaneState::Idle => palette.success(),
        PaneState::Unknown => palette.muted,
    }
}

/// Panes tile in the squarest grid that holds them.
fn columns_for(count: usize) -> usize {
    (count as f32).sqrt().ceil() as usize
}

fn leaf(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(
        || path.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leaf_of_a_path_names_the_folder() {
        assert_eq!(leaf("/Users/a/work/api"), "api");
        assert_eq!(leaf("/"), "/");
    }

    #[test]
    fn grid_columns_follow_the_square_root_of_the_pane_count() {
        assert_eq!(
            [1, 2, 3, 4, 5, 9, 10].map(columns_for),
            [1, 2, 2, 2, 3, 3, 4]
        );
    }
}

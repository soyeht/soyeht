//! A pane: the raised card, its header, and the body of its kind. The card is the only raised
//! surface; the header and everything under it are flat.

use super::ui::{
    OwnPress, Typeface, column, header_bar, hint, icon, id, pane_card_shell, row, rule,
};
use crate::{
    assets::Icon,
    open_menu::Menu,
    terminal_view,
    theme::{Palette, desktop, pane_interior, pane_layout, radius, space, type_scale},
    workbench::Workbench,
};
use gpui::{
    Anchor, AnyElement, ClickEvent, Context, Div, FontWeight, IntoElement, ParentElement,
    SharedString, Styled, div, prelude::FluentBuilder, px, rgb,
};
use gpui::{InteractiveElement, MouseButton, StatefulInteractiveElement};
use soyeht_client::{
    agent::{elapsed_text, status_word},
    terminal_state::{self, Severity},
};
use soyeht_protocol::{
    Pane, PaneMode, PaneState, WorkspacePaneLayoutAxis, client_policy::StateDot,
};
use std::time::Instant;

/// The buttons of the header, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Loop,
    Tasks,
    SplitColumns,
    SplitRows,
    Close,
}

impl Action {
    fn icon(self) -> Icon {
        match self {
            Self::Loop => Icon::Timer,
            Self::Tasks => Icon::List,
            Self::SplitColumns => Icon::Columns,
            Self::SplitRows => Icon::Rows,
            Self::Close => Icon::Close,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Loop => "loop",
            Self::Tasks => "tasks",
            Self::SplitColumns => "split-columns",
            Self::SplitRows => "split-rows",
            Self::Close => "close",
        }
    }

    /// The button's tooltip, named as on the Mac.
    fn title(self) -> &'static str {
        match self {
            Self::Loop => "Loop and lead controls",
            Self::Tasks => "Open workspace tasks",
            Self::SplitColumns => "Split pane into columns",
            Self::SplitRows => "Split pane into rows",
            Self::Close => "Close this pane",
        }
    }
}

fn actions(mode: &PaneMode) -> Vec<Action> {
    let mut all = Vec::new();
    if *mode == PaneMode::Agent {
        all.push(Action::Loop);
    }
    all.extend([
        Action::Tasks,
        Action::SplitColumns,
        Action::SplitRows,
        Action::Close,
    ]);
    all
}

impl Workbench {
    pub(super) fn pane_card(
        &mut self,
        pane: &Pane,
        focused: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane.pane_id.clone();
        // An editor has a header of its own: the file's path, not the pane's name and state.
        let (header, body) = match pane.mode {
            PaneMode::Editor => (
                self.editor_header(cx, pane, palette),
                self.editor_body(cx, palette, pane, focused),
            ),
            PaneMode::Terminal => (
                self.pane_header(pane, palette, cx),
                self.terminal_body(pane, focused, palette, cx),
            ),
            PaneMode::Agent => (
                self.pane_header(pane, palette, cx),
                self.agent_body(pane, palette, cx),
            ),
            _ => (
                self.pane_header(pane, palette, cx),
                notice_body(palette, mode_notice(&pane.mode)),
            ),
        };
        pane_card_shell(palette)
            .id(id("pane", &pane.pane_id))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| this.focus_pane(&pane_id, window, cx)),
            )
            .child(header)
            .child(rule(palette.well, desktop::HAIRLINE))
            .child(body)
            .into_any_element()
    }

    fn pane_header(
        &mut self,
        pane: &Pane,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let is_agent = pane.mode == PaneMode::Agent;
        let view = self.state.agent(&pane.pane_id);
        let needs_you = pane.state == PaneState::WaitingForYou
            || view.is_some_and(|view| view.permission.is_some());
        let session = pane.agent.as_ref();
        let status = status_word(
            &pane.state,
            session.is_some_and(|agent| !agent.auth_methods.is_empty()),
            session.is_some_and(|agent| agent.session_id.is_some()),
        );
        let ticker = view.and_then(|view| view.ticker(pane.title.as_deref()));
        let elapsed = view
            .filter(|_| pane.state.is_turn_running())
            .and_then(|view| view.turn.elapsed(Instant::now()))
            .map(elapsed_text);
        let buttons: Vec<AnyElement> = actions(&pane.mode)
            .into_iter()
            .map(|action| self.header_button(pane, action, palette, cx))
            .collect();
        header_bar(pane_layout::HEADER_HEIGHT)
            .font_weight(FontWeight::MEDIUM)
            .text_size(px(type_scale::CONTROL))
            .text_color(rgb(palette.secondary))
            .child(
                div()
                    .size(px(pane_layout::HEADER_STATE_DOT))
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(state_color(&pane.state, palette))),
            )
            .child(
                // The title gives way first, so the header buttons always fit a narrow pane.
                div()
                    .min_w_0()
                    .max_w(px(desktop::PANE_TITLE_MAX_WIDTH))
                    .truncate()
                    .font_weight(FontWeight::EXTRA_BOLD)
                    .text_size(px(type_scale::BODY))
                    .text_color(rgb(palette.ink))
                    .child(SharedString::from(pane.name.clone())),
            )
            .child(
                row()
                    .flex_1()
                    .min_w_0()
                    .gap(px(space::X4))
                    .pr(px(space::X12))
                    .child(
                        // The line is cut at the pane's width: the whole of it is in the tooltip.
                        div()
                            .id(id("ticker", &pane.pane_id))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(ticker.is_some(), |line| {
                                line.tooltip(hint(ticker.clone().unwrap_or_default(), palette))
                            })
                            .child(SharedString::from(ticker.unwrap_or_default())),
                    )
                    .children(elapsed.map(|elapsed| {
                        div()
                            .flex_none()
                            .mono()
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(type_scale::SMALL))
                            .child(format!("\u{b7} {elapsed}"))
                    })),
            )
            .when(is_agent, |header| {
                header.child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(type_scale::SMALL))
                        .text_color(rgb(if needs_you {
                            palette.accent
                        } else {
                            palette.secondary
                        }))
                        .child(SharedString::from(status)),
                )
            })
            .children(buttons)
    }

    /// A button of a pane's header; a header of its own kind (an editor's) draws the ones it shares.
    pub(super) fn header_button(
        &mut self,
        pane: &Pane,
        action: Action,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane.pane_id.clone();
        let key = id(action.name(), &pane.pane_id);
        // The loop button opens the loop panel.
        let menu = (action == Action::Loop).then(|| Menu::Loop(pane.pane_id.clone()));
        let open = menu.as_ref().is_some_and(|menu| self.menus.is_open(menu));
        if action == Action::Loop && !open {
            // The panel starts again from the pane's loop each time it opens.
            if let Some(ui) = self.panes.get_mut(&pane.pane_id) {
                ui.loop_form = None;
            }
        }
        let connected = self.state.connection == soyeht_client::Connection::Connected;
        let enabled = menu.is_some() || connected;
        let body = open.then(|| self.loop_panel(pane, palette, cx));
        div()
            .relative()
            .flex_none()
            .child(
                row()
                    .id(key)
                    .w(px(pane_layout::HEADER_ACTION_WIDTH))
                    .h(px(pane_layout::HEADER_HEIGHT))
                    .justify_center()
                    .rounded(px(radius::COMPACT))
                    .when(!enabled, |button| {
                        button.opacity(pane_interior::DISABLED_OPACITY)
                    })
                    .when(enabled, |button| {
                        button
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(palette.hover)))
                    })
                    .own_press()
                    .tooltip(hint(action.title(), palette))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(menu) = menu.clone() {
                            this.toggle_menu(menu, cx);
                        } else if connected {
                            match action {
                                Action::SplitColumns => this.split_pane(
                                    &pane_id,
                                    WorkspacePaneLayoutAxis::Horizontal,
                                    cx,
                                ),
                                Action::SplitRows => {
                                    this.split_pane(&pane_id, WorkspacePaneLayoutAxis::Vertical, cx)
                                }
                                Action::Close => this.close_pane(&pane_id),
                                Action::Tasks => this.open_tasks(cx),
                                Action::Loop => {}
                            }
                        }
                    }))
                    .child(icon(
                        action.icon(),
                        desktop::HEADER_ACTION_GLYPH,
                        palette.secondary,
                    )),
            )
            .children(body.map(|body| {
                super::menus::popup(
                    palette,
                    Anchor::TopRight,
                    (
                        pane_layout::HEADER_ACTION_WIDTH,
                        pane_layout::HEADER_HEIGHT + space::X4,
                    ),
                    body,
                    cx,
                )
            }))
            .into_any_element()
    }

    /// A terminal: the notice its state calls for above the screen, the screen, and under it the
    /// notice for output that was not recorded.
    fn terminal_body(
        &self,
        pane: &Pane,
        focused: bool,
        palette: &'static Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = pane.pane_id.clone();
        let gap = self
            .state
            .has_gap(&pane.pane_id)
            .then_some(terminal_state::RECORDED_GAP);
        column()
            .flex_1()
            .min_h_0()
            .children(
                terminal_state::notice(&pane.state, pane.exit_code)
                    .map(|notice| banner(notice, palette)),
            )
            .child(
                column()
                    .id(super::ui::id("terminal", &pane.pane_id))
                    .flex_1()
                    .min_h_0()
                    .px(px(space::X2))
                    .pt(px(space::X8))
                    .on_scroll_wheel(cx.listener({
                        let id = id.clone();
                        move |this, event, _, cx| this.scroll_terminal(&id, event, cx)
                    }))
                    .child(terminal_view::surface(cx.entity(), id, palette, focused)),
            )
            .children(gap.map(|notice| {
                div()
                    .flex_none()
                    .p(px(space::X10))
                    .child(notice_label(notice, palette))
            }))
            .into_any_element()
    }
}

/// The line above a terminal's screen: on the pane's face when it only informs, on a well when
/// something is wrong.
fn banner(notice: terminal_state::Notice, palette: &Palette) -> AnyElement {
    row()
        .flex_none()
        .px(px(space::X10))
        .py(px(space::X7))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(type_scale::CONTROL))
        .when(notice.severity == Severity::Warning, |line| {
            line.bg(rgb(palette.well))
        })
        .child(notice_label(notice, palette))
        .into_any_element()
}

/// A notice's icon and words, in the secondary ink or the warning color.
fn notice_label(notice: terminal_state::Notice, palette: &Palette) -> Div {
    let (glyph, color) = match notice.severity {
        Severity::Neutral => (Icon::Stopped, palette.secondary),
        Severity::Warning => (Icon::Alert, palette.warning()),
    };
    super::agent::labelled(glyph, color, notice.text)
}

fn notice_body(palette: &Palette, message: &str) -> AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .px(px(space::X18))
        .text_color(rgb(palette.muted))
        .child(SharedString::from(message.to_owned()))
        .into_any_element()
}

fn mode_notice(mode: &PaneMode) -> &'static str {
    match mode {
        PaneMode::Web => "Web panes are not available in the desktop client yet.",
        PaneMode::Writing => "Writing panes are not available in the desktop client yet.",
        PaneMode::Terminal | PaneMode::Agent | PaneMode::Editor => "",
    }
}

/// The color of the state dot: the palette role the contract gives the state (`x-clientPolicy`,
/// `paneStates`, which the Mac's pane header resolves too).
fn state_color(state: &PaneState, palette: &Palette) -> u32 {
    match state.dot() {
        StateDot::Success => palette.success(),
        StateDot::Accent => palette.accent,
        StateDot::Warning => palette.warning(),
        StateDot::Muted => palette.muted,
        StateDot::Secondary => palette.secondary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_have_the_loop_button_and_terminals_do_not() {
        assert_eq!(actions(&PaneMode::Agent)[0], Action::Loop);
        assert_eq!(actions(&PaneMode::Terminal)[0], Action::Tasks);
        assert_eq!(actions(&PaneMode::Agent).len(), 5);
        assert_eq!(*actions(&PaneMode::Terminal).last().unwrap(), Action::Close);
    }

    #[test]
    fn the_state_dot_follows_the_macs_colors() {
        let palette = crate::theme::ThemeName::NeoMilk.palette();
        assert_eq!(state_color(&PaneState::Working, palette), palette.success());
        assert_eq!(state_color(&PaneState::Idle, palette), palette.success());
        assert_eq!(
            state_color(&PaneState::WaitingForYou, palette),
            palette.accent
        );
        assert_eq!(
            state_color(&PaneState::NoSignal, palette),
            palette.warning()
        );
        assert_eq!(state_color(&PaneState::Dead, palette), palette.muted);
        assert_eq!(
            state_color(&PaneState::Cancelling, palette),
            palette.secondary
        );
    }

    #[test]
    fn the_header_buttons_are_named_as_the_mac_names_them() {
        let titles: Vec<_> = actions(&PaneMode::Agent)
            .iter()
            .map(|action| action.title())
            .collect();
        assert_eq!(
            titles,
            [
                "Loop and lead controls",
                "Open workspace tasks",
                "Split pane into columns",
                "Split pane into rows",
                "Close this pane"
            ]
        );
    }
}

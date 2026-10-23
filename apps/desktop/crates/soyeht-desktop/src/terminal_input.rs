//! The pointer and the clipboard in a terminal pane: selecting text, copying it, pasting, and
//! opening a web address. What a selection or an address is lives in `soyeht_client::screen`;
//! this turns the window's events into those calls.

use crate::{
    file_open::Files,
    open_menu::Menu,
    shortcuts,
    terminal_view::CellSize,
    workbench::{CopySelection, Workbench},
};
use std::time::Duration;

use gpui::{
    Bounds, ClipboardItem, Context, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    Point, Window,
};
use soyeht_client::{
    editor::keys::Action as EditorAction,
    keys,
    screen::{
        TerminalScreen,
        links::Link,
        selection::{Located, Span, locate},
    },
};
use soyeht_protocol::PaneMode;

/// How often a drag held past an edge scrolls.
const DRAG_SCROLL_INTERVAL: Duration = Duration::from_millis(50);

/// The cell the pointer is over in a drag, and the lines a step of the timer scrolls (toward
/// older lines when positive, none while the pointer is over the screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DragPoint {
    row: u16,
    column: u16,
    lines: i32,
}

/// What the window keeps for a terminal pane besides its screen.
#[derive(Default)]
pub(crate) struct TerminalUi {
    /// Where the screen was last drawn, and the size of a cell there.
    pub geometry: Option<(Bounds<Pixels>, CellSize)>,
    /// A drag that selects text is under way.
    pub dragging: bool,
    /// Which drag it is, so that the timer of one that ended does not scroll the next.
    pub drag_epoch: u32,
    /// Where the pointer is in the drag, and how far it scrolls each step: what the timer acts on
    /// while the pointer rests past an edge.
    pub drag_point: Option<DragPoint>,
    /// The address under the pointer while the link key is down.
    pub hover: Option<Link>,
    /// Where the context menu was opened, and the address that was under the pointer there.
    pub menu_at: Point<Pixels>,
    pub menu_link: Option<Link>,
}

/// Whether a click with these keys held opens the address under it: Control on Linux and
/// Windows, Command on the Mac (as in its terminals).
fn opens_links(modifiers: &Modifiers) -> bool {
    shortcuts::command_held(modifiers)
}

impl Workbench {
    /// Called while painting a terminal: where its screen is.
    pub fn record_terminal_geometry(
        &mut self,
        pane_id: &str,
        bounds: Bounds<Pixels>,
        cell: CellSize,
    ) {
        self.panes.of(pane_id).terminal.geometry = Some((bounds, cell));
    }

    /// The address under the pointer while the link key is down, for the painter to underline.
    pub fn hovered_link(&self, pane_id: &str) -> Option<Span> {
        self.panes
            .get(pane_id)?
            .terminal
            .hover
            .as_ref()
            .map(|link| link.span)
    }

    pub fn terminal_dragging(&self, pane_id: &str) -> bool {
        self.panes
            .get(pane_id)
            .is_some_and(|ui| ui.terminal.dragging)
    }

    fn terminal_cell(&self, pane_id: &str, at: Point<Pixels>) -> Option<Located> {
        let (bounds, cell) = self.panes.get(pane_id)?.terminal.geometry?;
        let grid = self.state.screen(pane_id)?.size();
        let pair = |x: Pixels, y: Pixels| (f32::from(x), f32::from(y));
        Some(locate(
            pair(at.x, at.y),
            pair(bounds.origin.x, bounds.origin.y),
            pair(cell.width, cell.height),
            grid,
        ))
    }

    pub(crate) fn terminal_mouse_down(
        &mut self,
        pane_id: &str,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(at) = self.terminal_cell(pane_id, event.position) else {
            return;
        };
        if opens_links(&event.modifiers)
            && let Some(link) = self.link_at_cell(pane_id, &at)
            && self.open_link(pane_id, &link, cx)
        {
            return;
        }
        let Some(screen) = self.state.screen_mut(pane_id) else {
            return;
        };
        if event.modifiers.shift && screen.has_selection() {
            screen.extend_selection(at.row, at.column);
        } else {
            screen.begin_selection(at.row, at.column, event.click_count);
        }
        let ui = &mut self.panes.of(pane_id).terminal;
        let started = !ui.dragging;
        ui.dragging = true;
        ui.drag_point = None;
        if started {
            ui.drag_epoch = ui.drag_epoch.wrapping_add(1);
            let epoch = ui.drag_epoch;
            self.scroll_while_dragging(pane_id, epoch, cx);
        }
        cx.notify();
    }

    /// While the pointer rests past an edge no event comes, so a timer keeps scrolling the drag
    /// and growing the selection until it ends.
    fn scroll_while_dragging(&self, pane_id: &str, epoch: u32, cx: &mut Context<Self>) {
        let pane_id = pane_id.to_owned();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DRAG_SCROLL_INTERVAL).await;
                let again = this.update(cx, |this, cx| this.step_drag_scroll(&pane_id, epoch, cx));
                if !matches!(again, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One step of the timer. Whether the drag it belongs to is still under way.
    fn step_drag_scroll(&mut self, pane_id: &str, epoch: u32, cx: &mut Context<Self>) -> bool {
        let Some(ui) = self.panes.get(pane_id).map(|ui| &ui.terminal) else {
            return false;
        };
        if !ui.dragging || ui.drag_epoch != epoch {
            return false;
        }
        if let Some(point) = ui.drag_point.filter(|point| point.lines != 0)
            && let Some(screen) = self.state.screen_mut(pane_id)
        {
            screen.scroll(point.lines);
            screen.extend_selection(point.row, point.column);
            cx.notify();
        }
        true
    }

    /// The pointer moved anywhere in the window while a drag was under way.
    pub(crate) fn terminal_drag(
        &mut self,
        pane_id: &str,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if !self.terminal_dragging(pane_id) {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.terminal_release(pane_id, cx);
            return;
        }
        let Some(at) = self.terminal_cell(pane_id, event.position) else {
            return;
        };
        self.panes.of(pane_id).terminal.drag_point = Some(DragPoint {
            row: at.row,
            column: at.column,
            lines: at.edge_scroll(),
        });
        if let Some(screen) = self.state.screen_mut(pane_id) {
            screen.extend_selection(at.row, at.column);
            cx.notify();
        }
    }

    pub(crate) fn terminal_release(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        if self.terminal_dragging(pane_id) {
            let ui = &mut self.panes.of(pane_id).terminal;
            ui.dragging = false;
            ui.drag_point = None;
            cx.notify();
        }
    }

    /// The pointer moved over the screen: with the link key down, the address under it is marked.
    pub(crate) fn terminal_hover(
        &mut self,
        pane_id: &str,
        (at, modifiers): (Point<Pixels>, &Modifiers),
        cx: &mut Context<Self>,
    ) {
        if self.terminal_dragging(pane_id) {
            return;
        }
        let hover = if opens_links(modifiers) {
            self.link_under((pane_id, at), cx)
        } else {
            None
        };
        let ui = &mut self.panes.of(pane_id).terminal;
        if ui.hover != hover {
            ui.hover = hover;
            cx.notify();
        }
    }

    /// The address under the pointer, or the reference whose file the window knows to be in the
    /// pane's workspace: the link to underline. A reference it has not asked about yet is not
    /// one until the disk answers, off the UI thread, and the window is drawn again.
    fn link_under(
        &mut self,
        (pane_id, at): (&str, Point<Pixels>),
        cx: &mut Context<Self>,
    ) -> Option<Link> {
        let link = self.link_at_point(pane_id, at)?;
        match &link.file {
            Some(reference) => {
                let files = self.terminal_files(pane_id, cx)?;
                files.resolve(reference).map(|_| link)
            }
            None => Some(link),
        }
    }

    /// The address or the file reference under the pointer, whether or not the file is there.
    fn link_at_point(&mut self, pane_id: &str, at: Point<Pixels>) -> Option<Link> {
        let cell = self.terminal_cell(pane_id, at)?;
        self.link_at_cell(pane_id, &cell)
    }

    /// The files of the workspace a terminal belongs to.
    fn terminal_files(&self, pane_id: &str, cx: &mut Context<Self>) -> Option<Files> {
        self.files_of(pane_id, self.markdown.probes.clone(), cx)
    }

    /// The address or the file reference at a cell, whether or not the file is there.
    fn link_at_cell(&mut self, pane_id: &str, at: &Located) -> Option<Link> {
        self.state.screen_mut(pane_id)?.link_at(at.row, at.column)
    }

    /// Opens a link: an address in the system browser, a file in the editor at its line once the
    /// disk says it is in the workspace. False when the link is a file the window knows is not
    /// there, so that the click is an ordinary one.
    fn open_link(&mut self, pane_id: &str, link: &Link, cx: &mut Context<Self>) -> bool {
        let Some(reference) = &link.file else {
            cx.open_url(&link.url);
            return true;
        };
        let Some(files) = self.terminal_files(pane_id, cx) else {
            return false;
        };
        if files.known_missing(reference) {
            return false;
        }
        files.open(reference);
        true
    }

    /// The secondary button: the menu of what can be done here.
    pub(crate) fn terminal_context_menu(
        &mut self,
        pane_id: &str,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let link = self.link_at_point(pane_id, at);
        let ui = &mut self.panes.of(pane_id).terminal;
        ui.menu_at = at;
        ui.menu_link = None;
        let menu = Menu::Terminal(pane_id.to_owned());
        let reference = link.as_ref().and_then(|link| link.file.clone());
        // A web address is a link at once; a reference is one once the disk says its file is there.
        if reference.is_none() {
            self.panes.of(pane_id).terminal.menu_link = link.clone();
        }
        self.toggle_menu(menu.clone(), cx);
        let (Some(link), Some(reference)) = (link, reference) else {
            return;
        };
        let Some(files) = self.terminal_files(pane_id, cx) else {
            return;
        };
        let pane_id = pane_id.to_owned();
        files.check(&reference, move |this, probed, cx| {
            // The menu was closed, or opened again somewhere else, while the disk answered.
            let open = this.menus.is_open(&menu);
            let ui = &mut this.panes.of(&pane_id).terminal;
            if probed.opening.is_some() && open && ui.menu_at == at {
                ui.menu_link = Some(link);
                cx.notify();
            }
        });
    }

    /// Copy: the selection of a terminal, or what an editor has selected.
    pub(crate) fn copy_selection(
        &mut self,
        _: &CopySelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane_id) = self.focused_editor() {
            self.editor_action(&pane_id, EditorAction::Copy, cx);
            return;
        }
        if self
            .state
            .focused_pane()
            .is_some_and(|pane| pane.mode == PaneMode::Agent)
            && self.copy_transcript(cx)
        {
            return;
        }
        let terminal = self
            .state
            .focused_pane()
            .filter(|pane| pane.mode == PaneMode::Terminal)
            .map(|pane| pane.pane_id.clone());
        match terminal {
            Some(pane_id) => self.copy_terminal(&pane_id, cx),
            // Another kind of pane keeps the key for its own copy.
            None => cx.propagate(),
        }
    }

    /// Puts the selected text of a terminal on the clipboard. With nothing selected the clipboard
    /// keeps what it has.
    pub(crate) fn copy_terminal(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        if let Some(text) = self
            .state
            .screen_mut(pane_id)
            .and_then(TerminalScreen::selected_text)
        {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Pastes the clipboard's text into a terminal.
    pub(crate) fn paste_terminal(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let bracketed = self
            .state
            .screen(pane_id)
            .is_some_and(TerminalScreen::bracketed_paste);
        self.send_input(pane_id, keys::paste(&text, bracketed), cx);
    }

    /// Opens the link the context menu was opened on.
    pub(crate) fn open_menu_link(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        if let Some(link) = self.panes.of(pane_id).terminal.menu_link.take() {
            self.open_link(pane_id, &link, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_key_is_control_off_the_mac_and_command_on_it() {
        let control = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        let command = Modifiers {
            platform: true,
            ..Modifiers::default()
        };
        assert!(!opens_links(&Modifiers::default()));
        assert_eq!(opens_links(&control), !cfg!(target_os = "macos"));
        assert_eq!(opens_links(&command), cfg!(target_os = "macos"));
    }
}

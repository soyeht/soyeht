//! The context menu of a terminal pane: copy the selection, paste, and open the address under
//! the pointer.

use super::menus;
use crate::{open_menu::Menu, shortcuts, theme::Palette, workbench::Workbench};
use gpui::{AnyElement, Context, IntoElement};

/// What a row of the menu does to the pane the menu is on.
type Act = fn(&mut Workbench, &str, &mut Context<Workbench>);

impl Workbench {
    /// The menu, when it is open on `pane_id`.
    pub(super) fn terminal_menu(
        &self,
        (pane_id, palette): (&str, &'static Palette),
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.menus.is_open(&Menu::Terminal(pane_id.to_owned())) {
            return None;
        }
        let ui = &self.panes.get(pane_id)?.terminal;
        let (at, has_link) = (ui.menu_at, ui.menu_link.is_some());
        let selected = self.state.screen(pane_id)?.has_selection();
        let connected = self.state.connection == soyeht_client::Connection::Connected;
        let mut rows = vec![
            row(
                (palette, pane_id),
                ("copy", "Copy", shortcuts::COPY.label()),
                (selected, Self::copy_terminal),
                cx,
            ),
            row(
                (palette, pane_id),
                ("paste", "Paste", shortcuts::PASTE.label()),
                (connected, Self::paste_terminal),
                cx,
            ),
        ];
        if has_link {
            rows.push(row(
                (palette, pane_id),
                ("open-link", "Open Link", String::new()),
                (true, Self::open_menu_link),
                cx,
            ));
        }
        Some(menus::popup_at(palette, at, menus::list(rows), cx))
    }
}

/// A row of the menu: its key, label and shortcut, and whether it can be used and what it does.
/// A row that cannot be used is drawn dimmed and does nothing.
fn row(
    (palette, pane_id): (&Palette, &str),
    (key, label, shortcut): (&str, &'static str, String),
    (enabled, act): (bool, Act),
    cx: &mut Context<Workbench>,
) -> AnyElement {
    let item = menus::command(
        palette,
        &format!("terminal-{key}"),
        label,
        shortcut,
        enabled,
    );
    if !enabled {
        return item.into_any_element();
    }
    let pane_id = pane_id.to_owned();
    menus::on_pick(item, cx, move |this, cx| {
        act(this, &pane_id, cx);
        this.close_menu(cx);
    })
}

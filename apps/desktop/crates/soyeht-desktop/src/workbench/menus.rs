//! The open menu, the rail and the place on show.

use super::Workbench;
use crate::{destination::Destination, open_menu::Menu};
use gpui::Context;
use std::time::Instant;

impl Workbench {
    pub(crate) fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        if self.menus.toggle(menu.clone(), Instant::now()) {
            // The popup of another control is measured again.
            crate::views::menus::control_forgotten(cx);
            if self.menus.is_open(&menu) {
                match &menu {
                    Menu::Harness(pane_id) => self.begin_switch(pane_id, cx),
                    Menu::Settings => self.begin_prefs(cx),
                    _ => {}
                }
            }
            cx.notify();
        }
    }

    pub(crate) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menus.close() {
            cx.notify();
        }
    }

    /// A click outside the open menu closed it.
    pub(crate) fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        // A list inside Preferences closes before the panel does.
        if self.prefs.mode_menu.take().is_some() {
            cx.notify();
            return;
        }
        if self.menus.dismiss(Instant::now()) {
            cx.notify();
        }
    }

    /// Opens or closes the "More harnesses" entry of the open harness picker.
    pub(crate) fn toggle_more_harnesses(&mut self, cx: &mut Context<Self>) {
        self.menus.toggle_more_harnesses();
        cx.notify();
    }

    pub(crate) fn toggle_rail(&mut self, cx: &mut Context<Self>) {
        self.rail_open = !self.rail_open;
        cx.notify();
    }

    pub(crate) fn select_destination(&mut self, destination: Destination, cx: &mut Context<Self>) {
        self.hide_start();
        self.destination = destination;
        if destination == Destination::Tasks {
            self.refresh_tasks();
        }
        cx.notify();
    }
}

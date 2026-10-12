//! Which popup menu is open. Only one is open at a time; the rules (a second click on the
//! button closes it, a click elsewhere closes it, Escape closes it, a harness picker's "More
//! harnesses" entry starts collapsed) are plain state, so they are unit tests and not
//! window behaviour.

use std::time::{Duration, Instant};

/// A popup menu: the control that opens it and what it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Menu {
    /// "+" in the top bar: what to open in the workspace.
    NewPane,
    /// "+N" in the top bar: the workspaces that did not fit.
    Overflow,
    /// The harness chip of an agent pane's composer.
    Harness(String),
    /// A model, effort or permission chip: the pane and the option.
    Config { pane_id: String, config_id: String },
    /// The loop button of an agent pane's header: the pane.
    Loop(String),
    /// The settings entry at the bottom of the rail.
    Settings,
    /// A header button the desktop client does not serve yet: the pane and the button.
    Notice {
        pane_id: String,
        button: &'static str,
    },
}

/// A click that closes a menu from outside lands before the click on its button. Within this
/// time the button's click is the same click and must not open the menu again.
const SAME_CLICK: Duration = Duration::from_millis(350);

#[derive(Debug, Default)]
pub struct OpenMenu {
    open: Option<Menu>,
    /// The "More harnesses" entry of the open harness picker is expanded.
    more_harnesses: bool,
    /// What a click elsewhere closed, and when.
    dismissed: Option<(Menu, Instant)>,
}

impl OpenMenu {
    pub fn is_open(&self, menu: &Menu) -> bool {
        self.open.as_ref() == Some(menu)
    }

    /// Whether "More harnesses" is expanded in the open menu.
    pub fn more_harnesses(&self) -> bool {
        self.open.is_some() && self.more_harnesses
    }

    /// The press on the control of `menu` at `now`: opens it, or closes it when it is the open
    /// one. A press that is the second half of the click that dismissed it changes nothing.
    /// Returns whether anything changed.
    pub fn toggle(&mut self, menu: Menu, now: Instant) -> bool {
        if let Some((closed, at)) = self.dismissed.take()
            && closed == menu
            && now.saturating_duration_since(at) < SAME_CLICK
        {
            return false;
        }
        self.more_harnesses = false;
        self.open = if self.is_open(&menu) {
            None
        } else {
            Some(menu)
        };
        true
    }

    /// Closes the open menu. Returns whether one was open.
    pub fn close(&mut self) -> bool {
        self.more_harnesses = false;
        self.open.take().is_some()
    }

    /// A click outside the open menu closed it at `now`.
    pub fn dismiss(&mut self, now: Instant) -> bool {
        self.more_harnesses = false;
        match self.open.take() {
            Some(menu) => {
                self.dismissed = Some((menu, now));
                true
            }
            None => false,
        }
    }

    /// Escape: closes the open menu. Returns whether there was one, that is, whether the key
    /// was this menu's and must not reach the terminal.
    pub fn escape(&mut self) -> bool {
        self.close()
    }

    /// Expands or collapses "More harnesses" in the open menu.
    pub fn toggle_more_harnesses(&mut self) {
        if self.open.is_some() {
            self.more_harnesses = !self.more_harnesses;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn harness() -> Menu {
        Menu::Harness("p1".into())
    }

    #[test]
    fn a_second_press_on_the_button_closes_the_menu() {
        let now = Instant::now();
        let mut menus = OpenMenu::default();
        assert!(menus.toggle(Menu::NewPane, now));
        assert!(menus.is_open(&Menu::NewPane));
        assert!(menus.toggle(Menu::NewPane, now));
        assert!(!menus.open.is_some());
    }

    #[test]
    fn opening_another_menu_replaces_the_open_one() {
        let now = Instant::now();
        let mut menus = OpenMenu::default();
        menus.toggle(Menu::NewPane, now);
        menus.toggle(Menu::Settings, now);
        assert!(menus.is_open(&Menu::Settings) && !menus.is_open(&Menu::NewPane));
    }

    #[test]
    fn the_press_that_follows_a_dismissing_click_does_not_reopen_the_menu() {
        let start = Instant::now();
        let mut menus = OpenMenu::default();
        menus.toggle(Menu::NewPane, start);
        assert!(menus.dismiss(start));
        assert!(!menus.toggle(Menu::NewPane, start + Duration::from_millis(50)));
        assert!(!menus.open.is_some());
        // The next press is a fresh one.
        assert!(menus.toggle(Menu::NewPane, start + Duration::from_millis(60)));
        assert!(menus.open.is_some());
    }

    #[test]
    fn a_late_press_or_another_button_opens_normally_after_a_dismissal() {
        let start = Instant::now();
        let mut menus = OpenMenu::default();
        menus.toggle(Menu::NewPane, start);
        menus.dismiss(start);
        assert!(menus.toggle(Menu::NewPane, start + SAME_CLICK * 2));
        assert!(menus.is_open(&Menu::NewPane));
        menus.dismiss(start + SAME_CLICK * 2);
        assert!(menus.toggle(Menu::Settings, start + SAME_CLICK * 2));
        assert!(menus.is_open(&Menu::Settings));
    }

    #[test]
    fn escape_closes_an_open_menu_and_leaves_the_key_to_the_terminal_otherwise() {
        let mut menus = OpenMenu::default();
        assert!(!menus.escape());
        menus.toggle(harness(), Instant::now());
        assert!(menus.escape());
        assert!(!menus.open.is_some());
        assert!(!menus.escape());
    }

    #[test]
    fn more_harnesses_starts_collapsed_and_only_belongs_to_the_open_menu() {
        let now = Instant::now();
        let mut menus = OpenMenu::default();
        menus.toggle_more_harnesses();
        assert!(!menus.more_harnesses(), "nothing is open to expand");
        menus.toggle(harness(), now);
        assert!(!menus.more_harnesses());
        menus.toggle_more_harnesses();
        assert!(menus.more_harnesses());
        menus.toggle_more_harnesses();
        assert!(!menus.more_harnesses());
    }

    #[test]
    fn closing_or_changing_the_menu_collapses_more_harnesses_again() {
        let now = Instant::now();
        let mut menus = OpenMenu::default();
        menus.toggle(harness(), now);
        menus.toggle_more_harnesses();
        menus.close();
        menus.toggle(harness(), now + SAME_CLICK * 2);
        assert!(!menus.more_harnesses());

        menus.toggle_more_harnesses();
        menus.toggle(Menu::NewPane, now);
        assert!(!menus.more_harnesses());

        menus.toggle_more_harnesses();
        menus.dismiss(now);
        menus.toggle(Menu::NewPane, now + SAME_CLICK * 2);
        assert!(!menus.more_harnesses());
    }
}

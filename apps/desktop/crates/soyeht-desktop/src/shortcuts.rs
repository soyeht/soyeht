//! The keyboard shortcuts of the window. Each one is a single value from which both the key
//! binding and the label the interface shows are made, so a label cannot say a key that is not
//! bound. This is also the one place that decides how the platform's command key is spelled:
//! Command on the Mac, Control elsewhere.

use gpui::Modifiers;

/// Whether this is the Mac, where Command is the command key and a text field moves with
/// Control+A and Control+E.
pub(crate) const ON_MAC: bool = cfg!(target_os = "macos");

/// Whether the platform's command key is held: Command on the Mac, Control elsewhere. The link
/// key of a terminal and the formats of prose use it.
pub(crate) fn command_held(modifiers: &Modifiers) -> bool {
    if ON_MAC {
        modifiers.platform
    } else {
        modifiers.control
    }
}

/// Who gets the key.
#[derive(Clone, Copy)]
enum Reach {
    /// The application: Command on the Mac, Control+Shift elsewhere, so that it never takes a key
    /// that a terminal program needs.
    Application,
    /// A text field or the editor: Command on the Mac, Control elsewhere.
    Text,
}

/// The key of a shortcut.
#[derive(Clone, Copy)]
enum Key {
    Letter(char),
    Enter,
}

#[derive(Clone, Copy)]
pub(crate) struct Shortcut {
    reach: Reach,
    shift: bool,
    key: Key,
}

pub(crate) const QUIT: Shortcut = Shortcut::application('q');
pub(crate) const OPEN_FOLDER: Shortcut = Shortcut::application('o');
pub(crate) const NEW_TERMINAL: Shortcut = Shortcut::application('t');
pub(crate) const PASTE: Shortcut = Shortcut::application('v');
pub(crate) const COPY: Shortcut = Shortcut::application('c');
/// Hides and shows the project panel of an editor (`soyeht_client::editor::keys`).
pub(crate) const PROJECT_PANEL: Shortcut = Shortcut::text(Key::Letter('e')).with_shift();
/// Creates the task of the new-task form, and creates and dispatches it.
pub(crate) const CREATE_TASK: Shortcut = Shortcut::text(Key::Enter);
pub(crate) const CREATE_AND_DISPATCH_TASK: Shortcut = Shortcut::text(Key::Enter).with_shift();

impl Shortcut {
    const fn application(letter: char) -> Self {
        Self {
            reach: Reach::Application,
            shift: false,
            key: Key::Letter(letter),
        }
    }

    const fn text(key: Key) -> Self {
        Self {
            reach: Reach::Text,
            shift: false,
            key,
        }
    }

    const fn with_shift(self) -> Self {
        Self {
            shift: true,
            ..self
        }
    }

    /// Whether Shift is part of the shortcut on this system.
    fn shifted(self) -> bool {
        self.shift || (matches!(self.reach, Reach::Application) && !ON_MAC)
    }

    /// The keystroke to bind, as GPUI spells it (`cmd-o`, `ctrl-shift-o`).
    pub(crate) fn binding(self) -> String {
        let command = if ON_MAC { "cmd" } else { "ctrl" };
        let shift = if self.shifted() { "-shift" } else { "" };
        let key = match self.key {
            Key::Letter(letter) => letter.to_string(),
            Key::Enter => "enter".to_owned(),
        };
        format!("{command}{shift}-{key}")
    }

    /// What the interface shows for it: `⌘O` on the Mac, `Ctrl+Shift+O` elsewhere.
    pub(crate) fn label(self) -> String {
        if ON_MAC {
            let key = match self.key {
                Key::Letter(letter) => letter.to_ascii_uppercase().to_string(),
                Key::Enter => "\u{21a9}".to_owned(),
            };
            format!(
                "{}\u{2318}{key}",
                if self.shifted() { "\u{21e7}" } else { "" }
            )
        } else {
            let key = match self.key {
                Key::Letter(letter) => letter.to_ascii_uppercase().to_string(),
                Key::Enter => "Enter".to_owned(),
            };
            format!("Ctrl+{}{key}", if self.shifted() { "Shift+" } else { "" })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Keystroke;
    use soyeht_client::editor::keys::{Action, Chord, action};

    const ALL: [Shortcut; 7] = [
        QUIT,
        OPEN_FOLDER,
        NEW_TERMINAL,
        PASTE,
        COPY,
        PROJECT_PANEL,
        CREATE_TASK,
    ];

    fn pressed(shortcut: Shortcut) -> Keystroke {
        Keystroke::parse(&shortcut.binding()).unwrap()
    }

    #[test]
    fn a_label_names_exactly_the_keys_of_its_binding() {
        for shortcut in ALL.into_iter().chain([CREATE_AND_DISPATCH_TASK]) {
            let pressed = pressed(shortcut);
            let label = shortcut.label();
            let modifiers = &pressed.modifiers;
            assert_eq!(modifiers.platform, ON_MAC, "{label}");
            assert_eq!(modifiers.control, !ON_MAC, "{label}");
            assert_eq!(
                label.contains(['\u{21e7}']) || label.contains("Shift"),
                modifiers.shift
            );
            let key_named = if pressed.key == "enter" {
                label.ends_with("Enter") || label.ends_with('\u{21a9}')
            } else {
                label.to_lowercase().ends_with(&pressed.key)
            };
            assert!(key_named, "{label} for {}", shortcut.binding());
        }
    }

    #[test]
    fn the_labels_read_as_the_menus_and_hints_have_always_said() {
        let on_mac = [
            (OPEN_FOLDER, "\u{2318}O"),
            (COPY, "\u{2318}C"),
            (PASTE, "\u{2318}V"),
            (PROJECT_PANEL, "\u{21e7}\u{2318}E"),
            (CREATE_TASK, "\u{2318}\u{21a9}"),
            (CREATE_AND_DISPATCH_TASK, "\u{21e7}\u{2318}\u{21a9}"),
        ];
        let elsewhere = [
            (OPEN_FOLDER, "Ctrl+Shift+O"),
            (COPY, "Ctrl+Shift+C"),
            (PASTE, "Ctrl+Shift+V"),
            (PROJECT_PANEL, "Ctrl+Shift+E"),
            (CREATE_TASK, "Ctrl+Enter"),
            (CREATE_AND_DISPATCH_TASK, "Ctrl+Shift+Enter"),
        ];
        for (shortcut, label) in if ON_MAC { on_mac } else { elsewhere } {
            assert_eq!(shortcut.label(), label);
        }
    }

    #[test]
    fn the_project_panel_binding_is_the_editors_own_key() {
        let pressed = pressed(PROJECT_PANEL);
        let chord = Chord {
            key: &pressed.key,
            text: None,
            control: pressed.modifiers.control,
            alt: pressed.modifiers.alt,
            shift: pressed.modifiers.shift,
            platform: pressed.modifiers.platform,
        };
        assert_eq!(action(&chord), Some(Action::ToggleProjectPanel));
    }

    #[test]
    fn the_command_key_is_the_one_the_bindings_use() {
        let held = pressed(COPY).modifiers;
        assert!(command_held(&Modifiers {
            shift: false,
            ..held
        }));
        assert!(!command_held(&Modifiers::default()));
    }
}

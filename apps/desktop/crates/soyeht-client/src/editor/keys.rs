//! What a key press means in an editor pane. The caller names keys the way GPUI does: `"enter"`,
//! `"left"`, `"f3"`, or the character itself, with the text the key types, if it types any.
//!
//! The key of shortcuts is Control on Linux and Windows and Command on the Mac. A word is moved
//! by Control or Alt, a line by Command, as each system's editors do.
//!
//! What a key types is not an `Action`: the platform's text input delivers it, so that a dead key
//! (`~` then `a` is `ã`), AltGr and an input method compose as they do in every other program
//! (`types_text`).

use super::selection::Motion;

#[derive(Debug, Clone, Copy, Default)]
pub struct Chord<'a> {
    pub key: &'a str,
    /// The text the key types with the current layout, if any.
    pub text: Option<&'a str>,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    /// The Command key of the Mac.
    pub platform: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Type(String),
    Backspace,
    Delete,
    /// Backspace or Delete over a whole word.
    DeleteWord {
        backward: bool,
    },
    /// A motion; with `extend` it grows the selection.
    Move {
        motion: Motion,
        extend: bool,
    },
    /// A page up or down; the view says how many lines a page is.
    Page {
        up: bool,
        extend: bool,
    },
    SelectAll,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    Save,
    Find,
    FindNext,
    FindPrevious,
    /// Closes the find bar.
    Escape,
    ToggleProjectPanel,
    CloseTab,
    NextTab,
    PreviousTab,
}

/// Whether the key press types text, which the platform's text input delivers instead of the key
/// event: the key carries text (no control character) and is not a shortcut. A press with Control
/// or Alt held is a shortcut, unless `prefers_text`, the platform's word that the text depends on
/// those modifiers: AltGr arrives as Control and Alt together on Windows, and a dead key is text
/// in the making. Enter, Tab, Backspace, the arrows and every shortcut stay key events.
pub fn types_text(chord: &Chord, prefers_text: bool) -> bool {
    chord
        .text
        .is_some_and(|text| !text.is_empty() && text.chars().all(|c| !c.is_control()))
        && !chord.platform
        && (prefers_text || !(chord.control || chord.alt))
}

/// What a key press asks of an editor, other than typing text (`types_text`).
pub fn action(chord: &Chord) -> Option<Action> {
    let command = chord.control || chord.platform;
    let word = chord.control || chord.alt;
    let extend = chord.shift;
    let step = |motion| Some(Action::Move { motion, extend });
    // Control and Alt together are AltGr on Windows, and what it types comes through the
    // platform's text input (`types_text`): never a shortcut, in a field or an editor.
    if chord.control && chord.alt && !chord.platform {
        return None;
    }
    match chord.key {
        "left" if chord.platform => step(Motion::LineStart),
        "left" if word => step(Motion::WordLeft),
        "left" => step(Motion::Left),
        "right" if chord.platform => step(Motion::LineEnd),
        "right" if word => step(Motion::WordRight),
        "right" => step(Motion::Right),
        "up" if chord.platform => step(Motion::DocumentStart),
        "up" => step(Motion::Up),
        "down" if chord.platform => step(Motion::DocumentEnd),
        "down" => step(Motion::Down),
        "home" if command => step(Motion::DocumentStart),
        "home" => step(Motion::LineStart),
        "end" if command => step(Motion::DocumentEnd),
        "end" => step(Motion::LineEnd),
        "pageup" if command => Some(Action::PreviousTab),
        "pagedown" if command => Some(Action::NextTab),
        "pageup" => Some(Action::Page { up: true, extend }),
        "pagedown" => Some(Action::Page { up: false, extend }),
        "enter" => Some(Action::Type("\n".to_owned())),
        "tab" if chord.control && !chord.alt => Some(if chord.shift {
            Action::PreviousTab
        } else {
            Action::NextTab
        }),
        "tab" if !command && !chord.shift => Some(Action::Type("\t".to_owned())),
        "backspace" if word => Some(Action::DeleteWord { backward: true }),
        "delete" if word => Some(Action::DeleteWord { backward: false }),
        "backspace" => Some(Action::Backspace),
        "delete" => Some(Action::Delete),
        "escape" => Some(Action::Escape),
        "f3" if chord.shift => Some(Action::FindPrevious),
        "f3" => Some(Action::FindNext),
        key if command => shortcut(key, chord.shift),
        _ => None,
    }
}

fn shortcut(key: &str, shift: bool) -> Option<Action> {
    Some(match key {
        "a" => Action::SelectAll,
        "c" => Action::Copy,
        "x" => Action::Cut,
        "v" => Action::Paste,
        "z" if shift => Action::Redo,
        "z" => Action::Undo,
        "y" => Action::Redo,
        "s" => Action::Save,
        "f" => Action::Find,
        "g" if shift => Action::FindPrevious,
        "g" => Action::FindNext,
        "e" if shift => Action::ToggleProjectPanel,
        "w" => Action::CloseTab,
        "]" | "}" if shift => Action::NextTab,
        "[" | "{" if shift => Action::PreviousTab,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press<'a>(key: &'a str, text: Option<&'a str>) -> Chord<'a> {
        Chord {
            key,
            text,
            ..Chord::default()
        }
    }

    fn command(key: &str) -> Chord<'_> {
        Chord {
            control: true,
            ..press(key, None)
        }
    }

    #[test]
    fn named_keys_edit_and_keys_with_text_are_left_to_the_platform() {
        assert_eq!(action(&press("a", Some("a"))), None);
        assert_eq!(action(&press("space", Some(" "))), None);
        assert_eq!(
            action(&press("enter", Some("\n"))),
            Some(Action::Type("\n".into()))
        );
        assert_eq!(action(&press("tab", None)), Some(Action::Type("\t".into())));
        assert_eq!(action(&press("backspace", None)), Some(Action::Backspace));
        assert_eq!(action(&press("delete", None)), Some(Action::Delete));
        assert_eq!(action(&press("f1", None)), None);
        assert_eq!(action(&press("a", Some("\u{1}"))), None);
    }

    #[test]
    fn control_or_alt_with_backspace_and_delete_take_a_word() {
        let control = Chord {
            control: true,
            ..press("backspace", None)
        };
        assert_eq!(
            action(&control),
            Some(Action::DeleteWord { backward: true })
        );
        let alt = Chord {
            alt: true,
            ..press("delete", None)
        };
        assert_eq!(action(&alt), Some(Action::DeleteWord { backward: false }));
    }

    #[test]
    fn arrows_move_and_shift_grows_the_selection() {
        let left = action(&press("left", None));
        assert_eq!(
            left,
            Some(Action::Move {
                motion: Motion::Left,
                extend: false
            })
        );
        let select = Chord {
            shift: true,
            ..press("down", None)
        };
        assert_eq!(
            action(&select),
            Some(Action::Move {
                motion: Motion::Down,
                extend: true
            })
        );
        let word = Chord {
            control: true,
            ..press("right", None)
        };
        assert_eq!(
            action(&word),
            Some(Action::Move {
                motion: Motion::WordRight,
                extend: false
            })
        );
        let mac_word = Chord {
            alt: true,
            ..press("left", None)
        };
        assert_eq!(
            action(&mac_word),
            Some(Action::Move {
                motion: Motion::WordLeft,
                extend: false
            })
        );
        let mac_line = Chord {
            platform: true,
            ..press("right", None)
        };
        assert_eq!(
            action(&mac_line),
            Some(Action::Move {
                motion: Motion::LineEnd,
                extend: false
            })
        );
        assert_eq!(
            action(&command("home")),
            Some(Action::Move {
                motion: Motion::DocumentStart,
                extend: false
            })
        );
        assert_eq!(
            action(&press("pagedown", None)),
            Some(Action::Page {
                up: false,
                extend: false
            })
        );
    }

    #[test]
    fn shortcuts_follow_the_system_key_and_never_type() {
        for (key, wanted) in [
            ("a", Action::SelectAll),
            ("c", Action::Copy),
            ("x", Action::Cut),
            ("v", Action::Paste),
            ("z", Action::Undo),
            ("y", Action::Redo),
            ("s", Action::Save),
            ("f", Action::Find),
            ("g", Action::FindNext),
            ("w", Action::CloseTab),
        ] {
            assert_eq!(action(&command(key)), Some(wanted.clone()), "{key}");
            let mac = Chord {
                platform: true,
                ..press(key, Some(key))
            };
            assert_eq!(action(&mac), Some(wanted), "{key}");
        }
        let redo = Chord {
            shift: true,
            ..command("z")
        };
        assert_eq!(action(&redo), Some(Action::Redo));
        let panel = Chord {
            shift: true,
            ..command("e")
        };
        assert_eq!(action(&panel), Some(Action::ToggleProjectPanel));
        assert_eq!(action(&command("q")), None);
        assert_eq!(action(&press("f3", None)), Some(Action::FindNext));
        let tabs = Chord {
            shift: true,
            ..command("tab")
        };
        assert_eq!(action(&tabs), Some(Action::PreviousTab));
        assert_eq!(action(&command("pagedown")), Some(Action::NextTab));
    }

    #[test]
    fn a_key_with_text_and_no_shortcut_modifier_types() {
        assert!(types_text(&press("a", Some("a")), false));
        assert!(types_text(&press("space", Some(" ")), false));
        // Shift only changes the letter.
        let shifted = Chord {
            shift: true,
            ..press("a", Some("A"))
        };
        assert!(types_text(&shifted, false));
        // A dead key and the letter it composes with carry the text the layout gives.
        assert!(types_text(&press("~", Some("~")), true));
        assert!(types_text(&press("a", Some("\u{e3}")), false));
        assert!(types_text(&press("c", Some("\u{e7}")), false));
    }

    #[test]
    fn keys_without_text_and_control_characters_do_not_type() {
        assert!(!types_text(&press("enter", None), false));
        assert!(!types_text(&press("backspace", None), false));
        assert!(!types_text(&press("left", None), false));
        assert!(!types_text(&press("f1", None), false));
        assert!(!types_text(&press("a", Some("")), false));
        assert!(!types_text(&press("a", Some("\u{1}")), false));
        assert!(!types_text(&press("tab", Some("\t")), false));
    }

    #[test]
    fn shortcuts_do_not_type_even_when_the_key_has_text() {
        let control = Chord {
            control: true,
            ..press("c", Some("c"))
        };
        assert!(!types_text(&control, false));
        // Alt+b is the key `b` with Alt held, not a `b`.
        let alt = Chord {
            alt: true,
            ..press("b", Some("b"))
        };
        assert!(!types_text(&alt, false));
        let command = Chord {
            platform: true,
            ..press("c", Some("c"))
        };
        assert!(!types_text(&command, false));
        assert!(!types_text(&command, true));
    }

    #[test]
    fn control_and_alt_together_are_never_a_shortcut() {
        // AltGr+z, AltGr+a and the rest of a Polish layout's letters are text, not undo and select
        // all: a field that stops the key would never type them.
        for key in ["a", "c", "x", "v", "y", "z", "s", "f", "left", "backspace"] {
            let altgr = Chord {
                control: true,
                alt: true,
                ..press(key, Some("\u{17c}"))
            };
            assert_eq!(action(&altgr), None, "{key}");
            let without_text = Chord {
                control: true,
                alt: true,
                ..press(key, None)
            };
            assert_eq!(action(&without_text), None, "{key}");
        }
    }

    #[test]
    fn altgr_types_what_the_layout_gives() {
        // On Windows AltGr arrives as Control and Alt together, and the platform says the text
        // depends on them.
        let altgr = Chord {
            control: true,
            alt: true,
            ..press("q", Some("/"))
        };
        assert!(types_text(&altgr, true));
        assert!(!types_text(&altgr, false));
        // Control and Alt with no text are the shortcut.
        let chord = Chord {
            control: true,
            alt: true,
            ..press("q", None)
        };
        assert!(!types_text(&chord, true));
    }
}

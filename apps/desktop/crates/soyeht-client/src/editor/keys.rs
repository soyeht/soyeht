//! What a key press means in an editor pane. The caller names keys the way GPUI does: `"enter"`,
//! `"left"`, `"f3"`, or the character itself, with the text the key types, if it types any.
//!
//! The key of shortcuts is Control on Linux and Windows and Command on the Mac. A word is moved
//! by Control or Alt, a line by Command, as each system's editors do.

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

/// The text a key types, when it is text: not a control character.
fn typed(text: Option<&str>) -> Option<String> {
    text.filter(|text| !text.is_empty() && text.chars().all(|c| !c.is_control() || c == '\t'))
        .map(str::to_owned)
}

pub fn action(chord: &Chord) -> Option<Action> {
    let command = chord.control || chord.platform;
    let word = chord.control || chord.alt;
    let extend = chord.shift;
    let step = |motion| Some(Action::Move { motion, extend });
    // AltGr arrives as Control and Alt together, and types.
    if chord.control && chord.alt && !chord.platform {
        return typed(chord.text).map(Action::Type);
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
        _ => typed(chord.text).map(Action::Type),
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
    fn plain_keys_type_and_named_keys_edit() {
        assert_eq!(
            action(&press("a", Some("a"))),
            Some(Action::Type("a".into()))
        );
        assert_eq!(
            action(&press("space", Some(" "))),
            Some(Action::Type(" ".into()))
        );
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
    fn altgr_types_what_the_layout_gives() {
        let altgr = Chord {
            control: true,
            alt: true,
            ..press("q", Some("@"))
        };
        assert_eq!(action(&altgr), Some(Action::Type("@".into())));
        let alt_only = Chord {
            alt: true,
            ..press("e", Some("\u{e9}"))
        };
        assert_eq!(action(&alt_only), Some(Action::Type("\u{e9}".into())));
    }
}

//! What a key press means in a text input, from the chord the editor pane already reads
//! (`editor::keys`): the same modifiers move by words and lines, and the same keys select all,
//! copy, cut, paste, undo and redo. The shortcut key is Control on Linux and Windows and Command
//! on the Mac. What the key types is not read here: the platform's text input delivers it.

use crate::editor::{
    keys::{Action, Chord, action},
    selection::Motion,
};

/// A key that edits or moves rather than types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// Return: send what is written.
    Submit,
    /// Shift+Return: a new line.
    Newline,
    /// Escape: close what is open.
    Dismiss,
    Backspace,
    Delete,
    DeleteWord {
        backward: bool,
    },
    Move {
        motion: Motion,
        extend: bool,
    },
    SelectAll,
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
}

/// The edit a chord asks for, or `None` for a key the input leaves alone: a key that types, a
/// shortcut of the application and the editor's own (save, find, tabs). With `emacs`, Control+A
/// and Control+E go to the start and the end of the line, as in a Mac text field.
pub fn edit(chord: &Chord, emacs: bool) -> Option<Edit> {
    if emacs && chord.control && !chord.alt && !chord.shift && !chord.platform {
        match chord.key {
            "a" => return Some(motion(Motion::LineStart)),
            "e" => return Some(motion(Motion::LineEnd)),
            _ => {}
        }
    }
    match chord.key {
        "enter" if chord.shift => return Some(Edit::Newline),
        "enter" => return Some(Edit::Submit),
        "escape" => return Some(Edit::Dismiss),
        _ => {}
    }
    Some(match action(chord)? {
        Action::Backspace => Edit::Backspace,
        Action::Delete => Edit::Delete,
        Action::DeleteWord { backward } => Edit::DeleteWord { backward },
        Action::Move { motion, extend } => Edit::Move { motion, extend },
        Action::SelectAll => Edit::SelectAll,
        Action::Copy => Edit::Copy,
        Action::Cut => Edit::Cut,
        Action::Paste => Edit::Paste,
        Action::Undo => Edit::Undo,
        Action::Redo => Edit::Redo,
        _ => return None,
    })
}

/// What a key asks of an open list of completions (the composer's "/" list), which takes these
/// keys while it is open: Up and Down move the choice, Return and Tab put it in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    Step(isize),
    Accept,
}

/// The completion a key press asks for, or `None` for a key the list leaves to the field.
/// Shift+Return still adds a line and Shift+Tab is not Tab.
pub fn completion(chord: &Chord) -> Option<Completion> {
    if chord.control || chord.alt || chord.shift || chord.platform {
        return None;
    }
    match chord.key {
        "up" => Some(Completion::Step(-1)),
        "down" => Some(Completion::Step(1)),
        "enter" | "tab" => Some(Completion::Accept),
        _ => None,
    }
}

fn motion(motion: Motion) -> Edit {
    Edit::Move {
        motion,
        extend: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: &str) -> Chord<'_> {
        Chord {
            key,
            ..Chord::default()
        }
    }

    fn control(key: &str) -> Chord<'_> {
        Chord {
            control: true,
            ..press(key)
        }
    }

    #[test]
    fn return_sends_and_shift_return_adds_a_line() {
        assert_eq!(edit(&press("enter"), false), Some(Edit::Submit));
        let shifted = Chord {
            shift: true,
            ..press("enter")
        };
        assert_eq!(edit(&shifted, false), Some(Edit::Newline));
        assert_eq!(edit(&press("escape"), false), Some(Edit::Dismiss));
    }

    #[test]
    fn altgr_letters_are_left_to_the_platform_in_a_field() {
        // Windows reports AltGr as Control and Alt: Polish "\u{17c}" is AltGr+z, which must not undo.
        for key in ["a", "c", "x", "v", "y", "z"] {
            let altgr = Chord {
                control: true,
                alt: true,
                ..press(key)
            };
            assert_eq!(edit(&altgr, false), None, "{key}");
        }
    }

    #[test]
    fn control_shortcuts_edit_the_text_and_plain_letters_are_left_to_the_platform() {
        for (key, wanted) in [
            ("a", Edit::SelectAll),
            ("c", Edit::Copy),
            ("x", Edit::Cut),
            ("v", Edit::Paste),
            ("z", Edit::Undo),
            ("y", Edit::Redo),
        ] {
            assert_eq!(edit(&control(key), false), Some(wanted), "{key}");
            let mac = Chord {
                platform: true,
                ..press(key)
            };
            assert_eq!(edit(&mac, true), Some(wanted), "mac {key}");
        }
        let redo = Chord {
            shift: true,
            ..control("z")
        };
        assert_eq!(edit(&redo, false), Some(Edit::Redo));
        assert_eq!(edit(&press("a"), false), None);
        assert_eq!(edit(&press("f1"), false), None);
        assert_eq!(edit(&press("tab"), false), None);
        // The editor's own shortcuts are not a text input's.
        assert_eq!(edit(&control("s"), false), None);
        assert_eq!(edit(&control("f"), false), None);
    }

    #[test]
    fn an_open_list_takes_the_arrows_return_and_tab_without_a_modifier() {
        assert_eq!(completion(&press("up")), Some(Completion::Step(-1)));
        assert_eq!(completion(&press("down")), Some(Completion::Step(1)));
        assert_eq!(completion(&press("enter")), Some(Completion::Accept));
        assert_eq!(completion(&press("tab")), Some(Completion::Accept));
        for modified in [
            Chord {
                shift: true,
                ..press("enter")
            },
            Chord {
                shift: true,
                ..press("tab")
            },
            Chord {
                alt: true,
                ..press("down")
            },
            control("up"),
        ] {
            assert_eq!(completion(&modified), None);
        }
        assert_eq!(completion(&press("left")), None);
    }

    #[test]
    fn control_a_and_e_move_to_the_line_ends_only_with_the_emacs_keys() {
        assert_eq!(
            edit(&control("a"), true),
            Some(Edit::Move {
                motion: Motion::LineStart,
                extend: false
            })
        );
        assert_eq!(
            edit(&control("e"), true),
            Some(Edit::Move {
                motion: Motion::LineEnd,
                extend: false
            })
        );
        assert_eq!(edit(&control("a"), false), Some(Edit::SelectAll));
    }

    #[test]
    fn arrows_move_by_character_word_and_line_and_shift_selects() {
        let word = Chord {
            control: true,
            shift: true,
            ..press("left")
        };
        assert_eq!(
            edit(&word, false),
            Some(Edit::Move {
                motion: Motion::WordLeft,
                extend: true
            })
        );
        assert_eq!(
            edit(&press("down"), false),
            Some(Edit::Move {
                motion: Motion::Down,
                extend: false
            })
        );
        assert_eq!(
            edit(&press("home"), false),
            Some(Edit::Move {
                motion: Motion::LineStart,
                extend: false
            })
        );
        let end = Chord {
            control: true,
            ..press("end")
        };
        assert_eq!(
            edit(&end, false),
            Some(Edit::Move {
                motion: Motion::DocumentEnd,
                extend: false
            })
        );
    }

    #[test]
    fn backspace_and_delete_take_a_character_or_with_control_a_word() {
        assert_eq!(edit(&press("backspace"), false), Some(Edit::Backspace));
        assert_eq!(edit(&press("delete"), false), Some(Edit::Delete));
        assert_eq!(
            edit(&control("backspace"), false),
            Some(Edit::DeleteWord { backward: true })
        );
        assert_eq!(
            edit(&control("delete"), false),
            Some(Edit::DeleteWord { backward: false })
        );
    }
}

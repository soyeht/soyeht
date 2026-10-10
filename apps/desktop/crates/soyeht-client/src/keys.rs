//! Turns a key press into the bytes a terminal program expects (xterm conventions). The
//! caller names keys the way GPUI does: `"enter"`, `"up"`, `"f5"`, or the character itself.

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Modifiers {
    /// The xterm modifier parameter: 1 plus shift (1), alt (2) and control (4).
    fn parameter(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.control)
    }

    fn any(self) -> bool {
        self.control || self.alt || self.shift
    }
}

/// `key_char` is the text the key produces with the current layout, if any.
/// `application_cursor` is the terminal's DECCKM mode.
pub fn encode(
    key: &str,
    key_char: Option<&str>,
    modifiers: Modifiers,
    application_cursor: bool,
) -> Option<Vec<u8>> {
    if let Some(bytes) = named(key, modifiers, application_cursor) {
        return Some(bytes);
    }
    if modifiers.control {
        let byte = control_byte(key)?;
        let mut bytes = Vec::with_capacity(2);
        if modifiers.alt {
            bytes.push(0x1b);
        }
        bytes.push(byte);
        return Some(bytes);
    }
    let text = key_char.filter(|text| !text.is_empty()).or_else(|| {
        let mut characters = key.chars();
        (characters.next().is_some() && characters.next().is_none()).then_some(key)
    })?;
    let mut bytes = Vec::with_capacity(text.len() + 1);
    if modifiers.alt {
        bytes.push(0x1b);
    }
    bytes.extend_from_slice(text.as_bytes());
    Some(bytes)
}

fn named(key: &str, modifiers: Modifiers, application_cursor: bool) -> Option<Vec<u8>> {
    let bytes = match key {
        "enter" => b"\r".to_vec(),
        "tab" if modifiers.shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" => b"\x1b".to_vec(),
        "backspace" if modifiers.alt => b"\x1b\x7f".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "space" if modifiers.control => vec![0],
        "up" | "down" | "right" | "left" | "home" | "end" => {
            let letter = match key {
                "up" => b'A',
                "down" => b'B',
                "right" => b'C',
                "left" => b'D',
                "home" => b'H',
                _ => b'F',
            };
            return Some(cursor_key(letter, modifiers, application_cursor));
        }
        "insert" => tilde(2, modifiers),
        "delete" => tilde(3, modifiers),
        "pageup" => tilde(5, modifiers),
        "pagedown" => tilde(6, modifiers),
        "f1" => ss3_or_csi(b'P', modifiers),
        "f2" => ss3_or_csi(b'Q', modifiers),
        "f3" => ss3_or_csi(b'R', modifiers),
        "f4" => ss3_or_csi(b'S', modifiers),
        "f5" => tilde(15, modifiers),
        "f6" => tilde(17, modifiers),
        "f7" => tilde(18, modifiers),
        "f8" => tilde(19, modifiers),
        "f9" => tilde(20, modifiers),
        "f10" => tilde(21, modifiers),
        "f11" => tilde(23, modifiers),
        "f12" => tilde(24, modifiers),
        _ => return None,
    };
    Some(bytes)
}

fn cursor_key(letter: u8, modifiers: Modifiers, application_cursor: bool) -> Vec<u8> {
    if modifiers.any() {
        format!("\x1b[1;{}{}", modifiers.parameter(), letter as char).into_bytes()
    } else if application_cursor {
        vec![0x1b, b'O', letter]
    } else {
        vec![0x1b, b'[', letter]
    }
}

fn tilde(code: u8, modifiers: Modifiers) -> Vec<u8> {
    if modifiers.any() {
        format!("\x1b[{code};{}~", modifiers.parameter()).into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn ss3_or_csi(letter: u8, modifiers: Modifiers) -> Vec<u8> {
    if modifiers.any() {
        format!("\x1b[1;{}{}", modifiers.parameter(), letter as char).into_bytes()
    } else {
        vec![0x1b, b'O', letter]
    }
}

fn control_byte(key: &str) -> Option<u8> {
    let mut characters = key.chars();
    let character = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    match character {
        'a'..='z' => Some(character as u8 - b'a' + 1),
        'A'..='Z' => Some(character as u8 - b'A' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '/' | '7' => Some(0x1f),
        '8' => Some(0x7f),
        _ => None,
    }
}

/// Wraps pasted text for programs that asked for bracketed paste. Escape characters are
/// dropped from the wrapped text, as terminals do: otherwise pasted text could carry its own
/// end marker and run the rest as typed commands.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let text = text.replace('\x1b', "");
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: &str, text: Option<&str>, modifiers: Modifiers) -> Option<Vec<u8>> {
        encode(key, text, modifiers, false)
    }

    const NONE: Modifiers = Modifiers {
        control: false,
        alt: false,
        shift: false,
    };
    const CONTROL: Modifiers = Modifiers {
        control: true,
        alt: false,
        shift: false,
    };
    const ALT: Modifiers = Modifiers {
        control: false,
        alt: true,
        shift: false,
    };
    const SHIFT: Modifiers = Modifiers {
        control: false,
        alt: false,
        shift: true,
    };

    #[test]
    fn printable_keys_send_their_text() {
        assert_eq!(press("a", Some("a"), NONE), Some(b"a".to_vec()));
        assert_eq!(press("a", Some("A"), SHIFT), Some(b"A".to_vec()));
        assert_eq!(press("space", Some(" "), NONE), Some(b" ".to_vec()));
        assert_eq!(
            press("e", Some("\u{e9}"), NONE),
            Some("\u{e9}".as_bytes().to_vec())
        );
    }

    #[test]
    fn control_letters_become_control_codes() {
        assert_eq!(press("c", None, CONTROL), Some(vec![3]));
        assert_eq!(press("d", Some("d"), CONTROL), Some(vec![4]));
        assert_eq!(press("[", None, CONTROL), Some(vec![0x1b]));
        assert_eq!(press("space", None, CONTROL), Some(vec![0]));
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(press("b", Some("b"), ALT), Some(b"\x1bb".to_vec()));
        assert_eq!(press("backspace", None, ALT), Some(b"\x1b\x7f".to_vec()));
    }

    #[test]
    fn editing_keys() {
        assert_eq!(press("enter", None, NONE), Some(b"\r".to_vec()));
        assert_eq!(press("backspace", None, NONE), Some(vec![0x7f]));
        assert_eq!(press("tab", None, NONE), Some(b"\t".to_vec()));
        assert_eq!(press("tab", None, SHIFT), Some(b"\x1b[Z".to_vec()));
        assert_eq!(press("delete", None, NONE), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn arrows_follow_the_application_cursor_mode() {
        assert_eq!(encode("up", None, NONE, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode("up", None, NONE, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            encode("left", None, CONTROL, true),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            encode("right", None, ALT, false),
            Some(b"\x1b[1;3C".to_vec())
        );
    }

    #[test]
    fn function_keys() {
        assert_eq!(press("f1", None, NONE), Some(b"\x1bOP".to_vec()));
        assert_eq!(press("f5", None, NONE), Some(b"\x1b[15~".to_vec()));
        assert_eq!(press("f12", None, SHIFT), Some(b"\x1b[24;2~".to_vec()));
    }

    #[test]
    fn unmapped_keys_send_nothing() {
        assert_eq!(press("shift", None, NONE), None);
        assert_eq!(press("f13", None, NONE), None);
    }

    #[test]
    fn paste_normalizes_newlines_and_brackets_on_request() {
        assert_eq!(paste("a\nb\r\nc", false), b"a\rb\rc".to_vec());
        assert_eq!(paste("x", true), b"\x1b[200~x\x1b[201~".to_vec());
    }

    #[test]
    fn bracketed_paste_cannot_close_itself_early() {
        let payload = "safe\x1b[201~\rrm -rf ~\r\x1b[200~";
        let sent = String::from_utf8(paste(payload, true)).unwrap();
        assert_eq!(sent, "\x1b[200~safe[201~\rrm -rf ~\r[200~\x1b[201~");
        assert_eq!(sent.matches("\x1b[201~").count(), 1);
        assert_eq!(sent.matches('\x1b').count(), 2);
    }
}

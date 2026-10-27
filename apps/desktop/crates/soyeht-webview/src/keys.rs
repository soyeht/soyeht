//! A key the window names, as the X11 key symbol the system web view reads. The symbols are the
//! ones of `X11/keysymdef.h`, which GTK and WebKit share on every window system.

/// An X11 key symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Keysym(pub u32);

const NAMED: [(&str, u32); 22] = [
    ("backspace", 0xff08),
    ("tab", 0xff09),
    ("enter", 0xff0d),
    ("escape", 0xff1b),
    ("home", 0xff50),
    ("left", 0xff51),
    ("up", 0xff52),
    ("right", 0xff53),
    ("down", 0xff54),
    ("pageup", 0xff55),
    ("pagedown", 0xff56),
    ("end", 0xff57),
    ("insert", 0xff63),
    ("delete", 0xffff),
    ("space", 0x20),
    ("f1", 0xffbe),
    ("f2", 0xffbf),
    ("f3", 0xffc0),
    ("f4", 0xffc1),
    ("f5", 0xffc2),
    ("f6", 0xffc3),
    ("f7", 0xffc4),
];
const FUNCTION_KEYS_FROM_F8: u32 = 0xffc5;

/// The symbol for the key a window reports by name (`"enter"`, `"left"`, `"f5"`) or by the
/// character it types. A key that types nothing and has no name here has no symbol.
pub fn keysym_for_key(key: &str, typed: Option<&str>) -> Option<Keysym> {
    if let Some((_, symbol)) = NAMED.iter().find(|(name, _)| *name == key) {
        return Some(Keysym(*symbol));
    }
    if let Some(rest) = key.strip_prefix('f')
        && let Ok(number) = rest.parse::<u32>()
        && (8..=12).contains(&number)
    {
        return Some(Keysym(FUNCTION_KEYS_FROM_F8 + number - 8));
    }
    let text = typed.filter(|text| !text.is_empty()).unwrap_or(key);
    let mut characters = text.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => Some(unicode_keysym(character)),
        _ => None,
    }
}

/// How X11 names a character: its own code in Latin-1, `0x01000000` plus its code elsewhere.
fn unicode_keysym(character: char) -> Keysym {
    let code = u32::from(character);
    if (0x20..0x7f).contains(&code) || (0xa0..=0xff).contains(&code) {
        Keysym(code)
    } else {
        Keysym(0x0100_0000 + code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_and_typed_characters_have_the_symbols_gtk_reads() {
        assert_eq!(keysym_for_key("enter", None), Some(Keysym(0xff0d)));
        assert_eq!(keysym_for_key("pagedown", None), Some(Keysym(0xff56)));
        assert_eq!(keysym_for_key("f5", None), Some(Keysym(0xffc2)));
        assert_eq!(keysym_for_key("f12", None), Some(Keysym(0xffc9)));
        assert_eq!(keysym_for_key("a", Some("a")), Some(Keysym(u32::from('a'))));
        assert_eq!(keysym_for_key("a", Some("A")), Some(Keysym(u32::from('A'))));
        assert_eq!(keysym_for_key("e", Some("é")), Some(Keysym(0xe9)));
        assert_eq!(
            keysym_for_key("x", Some("€")),
            Some(Keysym(0x0100_20ac)),
            "outside Latin-1 a character is named by its code"
        );
    }

    #[test]
    fn a_key_with_no_name_and_no_character_has_no_symbol() {
        assert_eq!(keysym_for_key("shift", None), None);
        assert_eq!(keysym_for_key("f13", None), None);
        assert_eq!(keysym_for_key("unidentified", Some("")), None);
    }
}

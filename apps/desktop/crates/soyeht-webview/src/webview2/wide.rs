//! Text as the system takes it: UTF-16 with a zero at the end.

use windows::core::PCWSTR;

/// `text` as UTF-16 with a zero at the end, for a call that takes a `PCWSTR`.
pub(super) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A `PCWSTR` for `units`, which [`wide`] made: valid while `units` is alive.
pub(super) fn pcwstr(units: &[u16]) -> PCWSTR {
    PCWSTR::from_raw(units.as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_ends_with_a_zero_and_keeps_what_is_outside_ascii() {
        assert_eq!(wide("a"), [97, 0]);
        assert_eq!(wide("é"), [0xe9, 0]);
        assert_eq!(wide(""), [0]);
    }
}

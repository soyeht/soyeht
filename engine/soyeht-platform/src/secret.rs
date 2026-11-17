//! A credential or cookie value that never prints, so it cannot reach a log by accident (the
//! logging rule of `log`: a line names a kind, never a value). Every crate that holds a token, a
//! key or a cookie holds it as a [`Secret`].

use std::fmt;

/// Upper bound for one token or cookie value.
pub const MAX_SECRET_BYTES: usize = 16 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Printable ASCII without whitespace or quotes, within the size bound: safe to place in a
    /// header without escaping.
    pub fn is_usable(&self) -> bool {
        let value = self.0.as_bytes();
        !value.is_empty()
            && value.len() <= MAX_SECRET_BYTES
            && value
                .iter()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_never_prints_and_is_usable_only_when_a_header_can_hold_it() {
        assert!(!format!("{:?}", Secret::new("sk-secret")).contains("secret"));
        assert!(Secret::new("sk-abc").is_usable());
        for bad in ["", "a b", "a\"b", "a\\b", "a\nb"] {
            assert!(!Secret::new(bad).is_usable(), "{bad:?}");
        }
        assert!(!Secret::new("k".repeat(MAX_SECRET_BYTES + 1)).is_usable());
    }
}

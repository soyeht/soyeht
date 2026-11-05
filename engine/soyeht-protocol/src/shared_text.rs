//! A text that is held once. A large document is read once and then handed to the store, the
//! editor's buffer and its syntax tree; each of them holds a clone of the same allocation, so the
//! text is in memory a single time however many places read it.

use std::{fmt, ops::Deref, sync::Arc};

/// An immutable text shared by its clones. Making one from a `String` keeps the string's
/// allocation, so it never copies the bytes.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SharedText(Arc<String>);

impl SharedText {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for SharedText {
    fn from(text: String) -> Self {
        Self(Arc::new(text))
    }
}

impl From<&str> for SharedText {
    fn from(text: &str) -> Self {
        Self::from(text.to_owned())
    }
}

impl Deref for SharedText {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for SharedText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<str> for SharedText {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for SharedText {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for SharedText {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl fmt::Debug for SharedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(formatter)
    }
}

impl fmt::Display for SharedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_one_allocation_and_a_string_is_not_copied() {
        let text = String::from("a large document");
        let address = text.as_ptr();
        let shared = SharedText::from(text);
        let clone = shared.clone();
        assert_eq!(clone.as_ptr(), address);
        assert_eq!(shared.as_ptr(), address);
        assert_eq!(&*clone, "a large document");
        assert_eq!(clone, shared);
    }
}

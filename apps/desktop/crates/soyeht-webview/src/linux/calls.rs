//! Asking the engine for something that answers later: a script's value, a picture of the page,
//! the cookies of an address. The answer arrives on the GTK thread and is sent to whoever waits.

use super::ffi::{Api, Ptr};
use crate::{Navigation, Result};
use std::{
    ffi::{CStr, CString, c_char},
    sync::mpsc::Sender,
};

/// What the caller waits for, and how to reach the library from the callback.
pub(super) struct Pending<T> {
    pub api: &'static Api,
    pub reply: Sender<Result<T>>,
}

impl<T> Pending<T> {
    /// Hands the pending call to a C callback as its user data.
    pub fn into_raw(self) -> Ptr {
        Box::into_raw(Box::new(self)).cast()
    }

    /// Takes the pending call back in the callback, which runs once.
    ///
    /// # Safety
    /// `raw` is a value made by [`Pending::into_raw`] that has not been taken yet.
    pub unsafe fn from_raw(raw: Ptr) -> Box<Self> {
        // SAFETY: the caller upholds the contract above.
        unsafe { Box::from_raw(raw.cast()) }
    }

    pub fn finish(self, value: Result<T>) {
        if self.reply.send(value).is_err() {
            // The caller timed out and left; there is nobody to tell.
        }
    }
}

/// What a navigation loads, with the address made into the form the library takes.
pub(super) type Target = crate::shared::Target<CString>;

/// The target of `navigation`, an address as the library takes it.
pub(super) fn target(navigation: Navigation<'_>) -> Result<Target> {
    Target::new(navigation, c_string)
}

/// Text for the library, which ends a string at a zero byte: text that holds one is refused.
pub(super) fn c_string(text: &str) -> Option<CString> {
    let Ok(text) = CString::new(text) else {
        return None;
    };
    Some(text)
}

/// A string the library owns, copied.
///
/// # Safety
/// `text` is null or points to a zero-terminated string that stays valid for the call.
pub(super) unsafe fn copy_string(text: *const c_char) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: not null, and zero-terminated by the caller's contract.
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_with_a_zero_byte_is_refused_and_other_text_is_kept() {
        assert!(c_string("a\0b").is_none());
        assert_eq!(
            c_string("https://example.com/").unwrap().to_bytes(),
            b"https://example.com/"
        );
    }
}

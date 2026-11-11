//! Linux and Windows have no handwriting recognizer yet: the refusal is typed, so a client
//! shows "not available on this computer" and does not retry.

use crate::{Error, Recognizer, Result};
use soyeht_protocol::InkUnsupportedReason;

pub(crate) fn system() -> Result<Box<dyn Recognizer>> {
    Err(Error::Unsupported(InkUnsupportedReason::NoOsSupport))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_system_without_a_recognizer_says_why() {
        assert_eq!(
            system().err(),
            Some(Error::Unsupported(InkUnsupportedReason::NoOsSupport))
        );
    }
}

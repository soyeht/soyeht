//! Systems the client has no notification for.

use super::{Error, Outcome, Toast};

pub(super) struct Shown;

pub(super) fn show(_toast: &Toast) -> Result<Shown, Error> {
    Err(Error::Unsupported)
}

impl Shown {
    pub(super) fn wait(self) -> Outcome {
        Outcome::Closed
    }
}

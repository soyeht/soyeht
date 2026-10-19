//! Where a failed open of a file is shown, as the Mac's `EditorOpenRefusal`. A file the engine
//! refuses as text is a state of the editor: the pane says why in place of the text, and no
//! problem banner opens. Every other failure of the open is the banner. The cases are
//! `contracts/vectors/editor.json` (`openFailure`), which both clients pass.

use crate::Error;
use soyeht_protocol::{
    ErrorKind,
    client_policy::{EDITOR_FILE_NOT_TEXT, EDITOR_FILE_TOO_LARGE},
};

/// A file the editor would not open, and what its pane says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The workspace path that was asked for.
    pub path: String,
    pub text: &'static str,
}

/// What the pane says for an error kind it shows; `None` for a kind that is the banner.
pub fn text(kind: &ErrorKind) -> Option<&'static str> {
    match kind {
        ErrorKind::FileNotText => Some(EDITOR_FILE_NOT_TEXT),
        ErrorKind::FileTooLarge => Some(EDITOR_FILE_TOO_LARGE),
        _ => None,
    }
}

/// What the pane says for `error`, when the engine refused the file as text.
pub(crate) fn of_error(error: &Error) -> Option<&'static str> {
    match error {
        Error::Remote(remote) => text(&remote.data.kind),
        _ => None,
    }
}

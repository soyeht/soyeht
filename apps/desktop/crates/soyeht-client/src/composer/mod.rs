//! What the composer of an agent pane decides, with no window in sight: what Return sends and
//! when, and how often the engine hears that the person is typing. The window reads the message
//! line and the staged files, asks here, and carries out the answer.

mod beat;
mod prompt;

pub use beat::DraftBeat;
pub use prompt::Submission;

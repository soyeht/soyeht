//! What an agent pane shows between its transcript and its composer, and how the transcript
//! nests subagents: the silence notice, the queue of messages waiting for the agent, the
//! sign-in card, the agent's questions and the subagent rows.
//!
//! The wording and the rules are the Mac's (`AgentSilencePresentation`, `AgentQueuePresentation`,
//! `AgentSignInState`, `AgentElicitationFields`, `AgentTranscriptGrouping`,
//! `AgentSubagentPresentation`); both clients pass the cases of `contracts/vectors/agent-cards.json`.

pub mod elicitation;
mod events;
pub mod queue;
pub mod sign_in;
pub mod silence;
mod state;
pub mod subagents;

pub use elicitation::{Field, fields};
pub use queue::{QueuePresentation, QueueRow, agent_message_name};
pub use sign_in::{SignInMethod, SignInState};
pub use silence::SilencePresentation;
pub use state::{CardState, Earlier};
pub use subagents::{
    Entry, Group, Status, SubagentPresentation, awaited, entries, preview, waiting,
};

/// The label of the button that loads the page of the transcript before the one held.
pub const LOAD_EARLIER: &str = "Load earlier messages";

#[cfg(test)]
mod vectors;

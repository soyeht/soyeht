//! The choices behind "Start with an agent": a folder, an agent and the first message. It is the
//! agent half of the Mac's new-pane form (`NewPaneForm`), and the cases both clients pass are in
//! `contracts/vectors/new-pane-form.json`: what stops the agent from starting, the first message
//! as it is sent, and which agent stays chosen when the list of agents changes.

use crate::harness_picker;
use soyeht_protocol::{HarnessInfo, limits::PROMPT_BYTES};

/// Why "Start" is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocker {
    NoFolder,
    NoAgent,
    AgentUnavailable,
    PromptTooLong,
}

impl Blocker {
    /// What to tell the person.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::NoFolder => "Choose a folder.",
            Self::NoAgent => "Choose an agent.",
            Self::AgentUnavailable => "This agent is not ready. Choose another or check it again.",
            Self::PromptTooLong => "The first message is too long.",
        }
    }
}

/// What the form holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewAgent {
    /// Absolute path of the folder the agent works in; its workspace is the one rooted there.
    pub directory: String,
    /// The id of the agent, empty while none is chosen.
    pub harness: String,
    /// The first message; blank sends nothing.
    pub prompt: String,
}

impl NewAgent {
    /// The first message as it is sent: without the blank space around it.
    pub fn first_prompt(&self) -> &str {
        self.prompt.trim()
    }

    /// What stops the agent from starting, or `None` when it can start.
    pub fn blocker(&self, harnesses: &[HarnessInfo]) -> Option<Blocker> {
        if self.directory.is_empty() {
            return Some(Blocker::NoFolder);
        }
        if self.harness.is_empty() {
            return Some(Blocker::NoAgent);
        }
        let ready = harnesses
            .iter()
            .any(|harness| harness.id == self.harness && harness.available);
        if !ready {
            return Some(Blocker::AgentUnavailable);
        }
        (self.first_prompt().len() > PROMPT_BYTES).then_some(Blocker::PromptTooLong)
    }

    /// Keeps the chosen agent while it is available; otherwise takes the preferred one or the one
    /// that was ready most recently. Does nothing while no agent can run.
    pub fn keep_or_choose_harness(&mut self, harnesses: &[HarnessInfo], preferred: Option<&str>) {
        let kept = harnesses
            .iter()
            .any(|harness| harness.id == self.harness && harness.available);
        if kept {
            return;
        }
        if let Some(first) = harness_picker::default_harness(harnesses, None, preferred) {
            self.harness.clone_from(&first.id);
        }
    }
}

#[cfg(test)]
mod tests;

//! What the client holds about an agent pane beyond its transcript, for the cards.

use soyeht_protocol::{AuthMethod, ElicitationRequest, Pane, PromptItem};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardState {
    /// The messages waiting for the agent, oldest first.
    pub queue: Vec<PromptItem>,
    /// The queue stopped after an interruption and waits for the person to resume it.
    pub queue_paused: bool,
    /// The question the agent is waiting on, if any.
    pub elicitation: Option<ElicitationRequest>,
    /// Where the page of the transcript before the oldest one held starts; none when the
    /// engine has nothing earlier.
    pub older: Option<String>,
    /// The ways to sign in, while the agent needs a sign-in.
    pub auth_methods: Vec<AuthMethod>,
    /// Why the last sign-in did not finish; cleared when the next one starts or succeeds.
    pub sign_in_failure: Option<String>,
}

/// What "Load earlier messages" does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Earlier {
    /// The client holds messages it has not drawn: draw more of them.
    Reveal,
    /// Everything held is drawn: ask the engine for the page that starts here.
    Fetch(String),
    Nothing,
}

impl CardState {
    /// What a snapshot says about a pane: the questions are read from the requests it lists.
    pub(crate) fn adopt(&mut self, pane: &Pane) {
        self.elicitation = None;
        self.queue_paused = pane
            .agent
            .as_ref()
            .and_then(|agent| agent.queue_paused)
            .unwrap_or(false);
        self.auth_methods = pane
            .agent
            .as_ref()
            .map(|agent| agent.auth_methods.clone())
            .unwrap_or_default();
    }

    /// Whether there is anything earlier than what is drawn, held (`hidden` messages) or at the
    /// engine.
    pub fn has_earlier(&self, hidden: usize) -> bool {
        hidden > 0 || self.older.is_some()
    }

    /// What loading earlier messages does when `hidden` messages are held and not drawn.
    pub fn earlier(&self, hidden: usize) -> Earlier {
        match (&self.older, hidden) {
            (_, 1..) => Earlier::Reveal,
            (Some(cursor), 0) => Earlier::Fetch(cursor.clone()),
            (None, 0) => Earlier::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earlier_messages_are_drawn_before_the_engine_is_asked_for_more() {
        let mut cards = CardState::default();
        assert!(!cards.has_earlier(0));
        assert_eq!(cards.earlier(0), Earlier::Nothing);
        assert_eq!(cards.earlier(7), Earlier::Reveal);
        cards.older = Some("40".into());
        assert!(cards.has_earlier(0));
        assert_eq!(cards.earlier(0), Earlier::Fetch("40".into()));
        assert_eq!(cards.earlier(7), Earlier::Reveal);
    }
}

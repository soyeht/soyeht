//! Recognizes the engine's own continuation prompt when a switched session
//! is replayed, without keeping the prompt's text.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Marks a session the engine started through a harness switch. The handover
/// marker takes the new agent's name from `State::launch`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct InternalContinuation {
    /// Identifies the continuation prompt without persisting its contents.
    /// Absent when the switch had no history to carry over.
    prompt: Option<PromptIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PromptIdentity {
    digest: String,
    bytes: usize,
}

/// Adapters may pad a replayed prompt; beyond this the message is not ours.
const REPLAY_WHITESPACE_SLACK_BYTES: usize = 4 * 1024;

fn prompt_digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.trim().as_bytes()))
}

impl InternalContinuation {
    pub(crate) fn record_prompt(&mut self, prompt: &str) {
        self.prompt = Some(PromptIdentity {
            digest: prompt_digest(prompt),
            bytes: prompt.trim().len(),
        });
    }

    pub(crate) fn has_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    /// Whether a replayed user message of this size could still be the prompt.
    pub(crate) fn could_match(&self, bytes: usize) -> bool {
        self.prompt
            .as_ref()
            .is_some_and(|prompt| bytes <= prompt.bytes + REPLAY_WHITESPACE_SLACK_BYTES)
    }

    /// Compares a fully assembled replayed user message, ignoring the
    /// surrounding whitespace adapters commonly trim or add.
    pub(crate) fn matches_replayed_prompt(&self, text: &str) -> bool {
        self.prompt
            .as_ref()
            .is_some_and(|prompt| prompt.digest == prompt_digest(text))
    }
}

#[cfg(test)]
mod continuation_tests {
    use super::InternalContinuation;

    #[test]
    fn continuation_identity_tolerates_trimming_and_does_not_store_the_prompt() {
        let mut continuation = InternalContinuation::default();
        continuation.record_prompt("[last messages] private context\n");
        assert!(continuation.matches_replayed_prompt("[last messages] private context"));
        assert!(continuation.matches_replayed_prompt("  [last messages] private context\n\n"));
        assert!(!continuation.matches_replayed_prompt("[last messages] other context"));
        assert!(!continuation.matches_replayed_prompt("[last messages]"));
        assert!(continuation.could_match(31));
        assert!(!continuation.could_match(31 + 4097));
        let persisted = serde_json::to_string(&continuation).unwrap();
        assert!(!persisted.contains("private context"));
    }
}

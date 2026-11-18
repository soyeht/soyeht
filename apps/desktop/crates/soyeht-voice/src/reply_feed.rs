//! The agent's replies as the voice loop takes them: each message has an identity, grows by
//! updates, and comes out as the sentences to speak (contract 20, section 3).

use crate::chunker::SentenceChunker;
use crate::speech_text::speakable;
use soyeht_protocol::client_policy::VOICE_REPLIES_REMEMBERED;
use std::collections::HashMap;

struct Reply {
    chunker: SentenceChunker,
    is_final: bool,
    cancelled: bool,
}

/// What an update of a reply gives: the sentences that became whole, and whether the reply is still
/// wanted. The words of a reply that was cancelled are neither, and say nothing of the agent's
/// current turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyUpdate {
    pub sentences: Vec<String>,
    pub accepted: bool,
}

/// - A reply is a message: updates with the same `id` are one reply, and another `id` is another
///   reply, spoken after the one before it, whether or not a voice turn preceded it.
/// - A reply that was cancelled is never spoken again: updates that still arrive for its `id` are
///   dropped, so the late words of a cancelled answer are not taken for the next one.
pub struct ReplyFeed {
    template: SentenceChunker,
    replies: HashMap<String, Reply>,
    /// The identities in the order they were first seen: the oldest is forgotten first.
    order: Vec<String>,
}

impl Default for ReplyFeed {
    fn default() -> Self {
        Self::new(SentenceChunker::default())
    }
}

impl ReplyFeed {
    pub fn new(sentences: SentenceChunker) -> Self {
        Self {
            template: sentences,
            replies: HashMap::new(),
            order: Vec::new(),
        }
    }

    pub fn update(&mut self, id: &str, markdown: &str, is_final: bool) -> ReplyUpdate {
        let reply = self.replies.entry(id.to_owned()).or_insert_with(|| Reply {
            chunker: self.template.clone(),
            is_final: false,
            cancelled: false,
        });
        if reply.cancelled {
            return ReplyUpdate {
                sentences: Vec::new(),
                accepted: false,
            };
        }
        let sentences = reply
            .chunker
            .update(&speakable(markdown, !is_final), is_final);
        reply.is_final = reply.is_final || is_final;
        if !self.order.iter().any(|seen| seen == id) {
            self.order.push(id.to_owned());
            if self.order.len() > remembered() {
                let oldest = self.order.remove(0);
                self.replies.remove(&oldest);
            }
        }
        ReplyUpdate {
            sentences,
            accepted: true,
        }
    }

    /// Every reply seen so far is cancelled.
    pub fn cancel(&mut self) {
        for reply in self.replies.values_mut() {
            reply.cancelled = true;
        }
    }

    /// Some reply that was not cancelled has more to come.
    pub fn has_pending(&self) -> bool {
        self.replies
            .values()
            .any(|reply| !reply.is_final && !reply.cancelled)
    }
}

fn remembered() -> usize {
    usize::try_from(VOICE_REPLIES_REMEMBERED).unwrap_or(usize::MAX)
}

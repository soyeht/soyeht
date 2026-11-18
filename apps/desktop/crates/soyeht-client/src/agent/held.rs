//! How much of a long conversation a replica holds (ADR 0167). The engine keeps the whole
//! transcript and pages it, so a client that lets go of the oldest messages can fetch them again
//! with "Load earlier"; what a pane costs follows what is on screen, not how long it has run.
//!
//! The rule is `window` of `contracts/vectors/agent-transcript.json`, which the Mac client passes
//! as well.

use super::AgentView;
use soyeht_protocol::{
    ToolStatus, TranscriptKind, TranscriptMessage,
    client_policy::{CAPS_HELD_MESSAGES, CAPS_OFFSCREEN_MESSAGES},
};

/// The sizes of the window: a pane that is shown and one that is not.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub held: usize,
    pub offscreen: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            held: CAPS_HELD_MESSAGES,
            offscreen: CAPS_OFFSCREEN_MESSAGES,
        }
    }
}

/// What the client keeps of a message: a tool call's content and raw input and output are trees
/// no view reads, and they are the largest part of a long run's tool calls.
pub(super) fn light(mut message: TranscriptMessage) -> TranscriptMessage {
    if let Some(tool) = &mut message.tool {
        tool.content = None;
        tool.raw_input = None;
        tool.raw_output = None;
    }
    message
}

/// The index before which a message is let go of, unless it is still needed: the newest `limit`
/// messages are kept, and a person who scrolled back also keeps the half of a window before the
/// message in view. A message in view that is not held counts as the newest.
fn start(messages: &[TranscriptMessage], limit: usize, viewing: Option<&str>) -> usize {
    let newest = messages.len().saturating_sub(limit);
    let at = viewing.and_then(|id| messages.iter().position(|held| held.local_id == id));
    at.map_or(newest, |index| newest.min(index.saturating_sub(limit / 2)))
}

/// Whether the pane still needs a message that is old enough to go: a reply or a subagent that
/// has not finished, a tool call that has not, and the newest usage report (the context ring
/// reads it). A command's output is never final in a live update, so it does not hold one.
fn is_needed(message: &TranscriptMessage, newest_usage: bool) -> bool {
    if message.kind == TranscriptKind::Usage {
        return newest_usage;
    }
    if let Some(tool) = &message.tool
        && !matches!(tool.status, ToolStatus::Completed | ToolStatus::Failed)
    {
        return true;
    }
    !message.complete && message.kind != TranscriptKind::Terminal
}

impl AgentView {
    /// Whether the conversation is on screen. A pane that is not shown keeps the small window,
    /// and what it holds beyond it goes at once.
    pub fn set_shown(&mut self, shown: bool) {
        if self.shown != shown {
            self.shown = shown;
            self.trim(Limits::default());
        }
    }

    /// The message the person is looking at, none at the newest.
    pub fn set_viewing(&mut self, viewing: Option<&str>) {
        if self.viewing.as_deref() != viewing {
            self.viewing = viewing.map(str::to_owned);
        }
    }

    /// Lets go of the oldest messages beyond the window, and points "Load earlier" at the oldest
    /// message kept after the last one let go of, so everything dropped can be fetched again.
    pub(super) fn trim(&mut self, limits: Limits) {
        let limit = if self.shown {
            limits.held
        } else {
            limits.offscreen
        };
        if self.messages.len() <= limit {
            return;
        }
        let viewing = self.viewing.as_deref().filter(|_| self.shown);
        let start = start(&self.messages, limit, viewing);
        if start == 0 {
            return;
        }
        let newest_usage = self
            .messages
            .iter()
            .rposition(|message| message.kind == TranscriptKind::Usage);
        let mut after_let_go = false;
        let mut cursor = None;
        for (index, message) in std::mem::take(&mut self.messages).into_iter().enumerate() {
            if index < start && !is_needed(&message, newest_usage == Some(index)) {
                self.revisions.remove(&message.local_id);
                after_let_go = true;
                continue;
            }
            if std::mem::take(&mut after_let_go) {
                cursor = Some(message.seq.clone());
            }
            self.messages.push(message);
        }
        if cursor.is_some() {
            self.cards.older = cursor;
        }
    }
}

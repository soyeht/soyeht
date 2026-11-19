//! How much of a long conversation a replica holds (ADR 0167). The engine keeps the whole
//! transcript and pages it, so a client that lets go of the oldest messages can fetch them again
//! with "Load earlier"; what a pane costs follows what is on screen, not how long it has run.
//!
//! The rule is `window` and `cursor` of `contracts/vectors/agent-transcript.json`, which the Mac
//! client passes as well.

use super::AgentView;
use crate::Result;
use soyeht_protocol::{
    TranscriptKind, TranscriptMessage, TranscriptRole,
    client_policy::{CAPS_HELD_MESSAGES, CAPS_OFFSCREEN_MESSAGES, PAGE_SIZE_TRANSCRIPT},
};

/// The sizes of the window: a pane that is shown and one that is not. A merge trims only past
/// `slack` more, back down to the size, so a streaming pane does not rebuild its transcript for
/// each message; leaving the screen trims at once.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub held: usize,
    pub offscreen: usize,
    pub slack: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            held: CAPS_HELD_MESSAGES,
            offscreen: CAPS_OFFSCREEN_MESSAGES,
            slack: PAGE_SIZE_TRANSCRIPT as usize,
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
/// message where they were, in view or left behind. A message that is not held counts as the
/// newest.
fn start(messages: &[TranscriptMessage], limit: usize, viewing: Option<&str>) -> usize {
    let newest = messages.len().saturating_sub(limit);
    let at = viewing.and_then(|id| messages.iter().position(|held| held.local_id == id));
    at.map_or(newest, |index| newest.min(index.saturating_sub(limit / 2)))
}

/// Whether the pane still needs a message that is old enough to go: in the latest turn (after the
/// person's last message), a reply or subagent that has not finished and a tool call that has not,
/// and anywhere the newest usage report (the context ring reads it). What an earlier turn left
/// unfinished is stale, and a command's output is never final in a live update.
fn is_needed(message: &TranscriptMessage, newest_usage: bool, in_latest_turn: bool) -> bool {
    if message.kind == TranscriptKind::Usage {
        return newest_usage;
    }
    if !in_latest_turn {
        return false;
    }
    if let Some(tool) = &message.tool {
        return !super::finished(&tool.status);
    }
    !message.complete && message.kind != TranscriptKind::Terminal
}

/// The cursor of "Load earlier" after a page: the older of the one held and the page's, so a
/// reload of the newest page does not move it past messages the replica does not hold.
fn older(held: Option<&str>, page: Option<String>) -> Option<String> {
    let (page, held) = (page?, held);
    match (
        held.and_then(|held| held.parse::<u64>().ok()),
        page.parse::<u64>(),
    ) {
        (Some(held_seq), Ok(page_seq)) if held_seq <= page_seq => held.map(str::to_owned),
        _ => Some(page),
    }
}

impl AgentView {
    /// Whether the conversation is on screen. A pane that is not shown keeps the small window,
    /// and what it holds beyond it goes at once.
    pub fn set_shown(&mut self, shown: bool) {
        if self.shown != shown {
            self.shown = shown;
            self.refilled = false;
            self.trim(Limits::default(), true);
        }
    }

    pub(crate) fn is_shown(&self) -> bool {
        self.shown
    }

    /// The message the person is looking at, none at the newest. It stays when the pane leaves
    /// the screen: it is where the person comes back to.
    pub fn set_viewing(&mut self, viewing: Option<&str>) {
        if self.viewing.as_deref() != viewing {
            self.viewing = viewing.map(str::to_owned);
        }
    }

    /// Whether a pane that is shown holds less than a page while the engine has more: it reads the
    /// newest page, once each time it is shown.
    pub(crate) fn take_refill(&mut self) -> bool {
        let wanted = self.shown
            && !self.refilled
            && self.cards.older.is_some()
            && self.messages.len() < PAGE_SIZE_TRANSCRIPT as usize;
        self.refilled |= wanted;
        wanted
    }

    /// Folds in a page of the transcript that `next` is the cursor before.
    pub(crate) fn merge_transcript(
        &mut self,
        page: Vec<TranscriptMessage>,
        next: Option<String>,
    ) -> Result<()> {
        self.cards.older = older(self.cards.older.as_deref(), next);
        self.merge_page(page)
    }

    /// Lets go of the oldest messages beyond the window, and points "Load earlier" at the oldest
    /// message kept after the last one let go of, so everything dropped can be fetched again.
    /// Unless `force`, only a pane that grew past its window and a page more is trimmed.
    pub(super) fn trim(&mut self, limits: Limits, force: bool) {
        let limit = if self.shown {
            limits.held
        } else {
            limits.offscreen
        };
        if self.messages.len() <= limit + if force { 0 } else { limits.slack } {
            return;
        }
        let start = start(&self.messages, limit, self.viewing.as_deref());
        if start == 0 {
            return;
        }
        let newest_usage = self
            .messages
            .iter()
            .rposition(|message| message.kind == TranscriptKind::Usage);
        let turn_start = self
            .messages
            .iter()
            .rposition(|message| {
                message.role == TranscriptRole::User && message.kind == TranscriptKind::Message
            })
            .unwrap_or(0);
        let mut after_let_go = false;
        let mut cursor = None;
        for (index, message) in std::mem::take(&mut self.messages).into_iter().enumerate() {
            if index < start
                && !is_needed(&message, newest_usage == Some(index), index >= turn_start)
            {
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

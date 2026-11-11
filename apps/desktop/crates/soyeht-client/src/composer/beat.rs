//! Telling the engine that a composer holds a draft, so that a loop never sends over it.

use crate::loops::draft_beat_due;
use std::time::Instant;

/// When the draft of one pane's composer was last announced to the engine.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DraftBeat {
    last: Option<Instant>,
}

impl DraftBeat {
    /// Whether the draft is to be announced now. An empty composer forgets the last announcement,
    /// so the next draft is announced at once; a dead connection announces nothing and is asked
    /// again at the next beat.
    pub fn due(&mut self, has_draft: bool, connected: bool, now: Instant) -> bool {
        if !has_draft {
            self.last = None;
            return false;
        }
        if draft_beat_due(has_draft, self.last, now) && connected {
            self.last = Some(now);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loops::DRAFT_BEAT;

    #[test]
    fn a_draft_is_announced_at_once_and_then_each_beat() {
        let (mut beat, start) = (DraftBeat::default(), Instant::now());
        assert!(beat.due(true, true, start));
        assert!(!beat.due(true, true, start + DRAFT_BEAT / 2));
        assert!(beat.due(true, true, start + DRAFT_BEAT));
    }

    #[test]
    fn an_empty_composer_forgets_the_last_announcement() {
        let (mut beat, start) = (DraftBeat::default(), Instant::now());
        assert!(beat.due(true, true, start));
        assert!(!beat.due(false, true, start + DRAFT_BEAT / 4));
        assert!(beat.due(true, true, start + DRAFT_BEAT / 2), "a new draft");
    }

    #[test]
    fn a_dead_connection_announces_nothing_and_keeps_the_draft_due() {
        let (mut beat, start) = (DraftBeat::default(), Instant::now());
        assert!(!beat.due(true, false, start));
        assert!(beat.due(true, true, start + DRAFT_BEAT / 4));
    }
}
